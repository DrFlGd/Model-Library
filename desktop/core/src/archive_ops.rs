//! Reviewed ZIP operations for a single existing model. Outputs are staged,
//! read back and hashed before they are published. Source cleanup is opt-in,
//! kept in the existing journal recovery directory and reversible.
use crate::{archive, index::Index, library::Library, model, relayout};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, HashSet}, fs::{self, File}, io::{Read, Write}, path::{Path, PathBuf}, sync::atomic::{AtomicBool, Ordering}};

const MAX_FILES: usize = 20_000;
const MAX_EXPANDED: u64 = 8 * 1024 * 1024 * 1024;
const MANIFEST: &str = "_model-library/archive-manifest.json";
fn field<'a>(v: &'a Value, k: &str) -> &'a str { v[k].as_str().unwrap_or("") }
fn stopped(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) { bail!("Stopped. The original files are still safe."); }
    Ok(())
}
fn safe(root: &Path, rel: &str) -> Result<PathBuf> {
    let path = archive::safe_entry(rel)?;
    if fs::symlink_metadata(root).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("A linked model directory is not a safe archive destination.");
    }
    let mut at = root.to_path_buf();
    for c in path.components() {
        at.push(c);
        if fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("Cannot read or write through a linked file or folder: {rel}");
        }
    }
    Ok(at)
}
fn approved_root(lib: &Library, root: &Path) -> Result<()> {
    let trusted = fs::canonicalize(lib.root())?;
    let actual = fs::canonicalize(root)?;
    if !actual.starts_with(&trusted) {
        bail!("The model directory resolves outside the approved library root.");
    }
    Ok(())
}
fn hash_stream<R: Read>(mut reader: R, max: u64) -> Result<(String,u64)> {
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buf = [0; 65536];
    loop {
        let n = reader.read(&mut buf).context("Archive content could not be read (corrupt or encrypted).")?;
        if n == 0 { break; }
        count = count.checked_add(n as u64).ok_or_else(|| anyhow!("Archive entry size overflow."))?;
        if count > max { bail!("Expanded file exceeds the safe size limit."); }
        hash.update(&buf[..n]);
    }
    Ok((hex::encode(hash.finalize()), count))
}
fn file_hash(p: &Path) -> Result<(String,u64)> {
    let f = File::open(p).with_context(|| format!("Cannot open {}", p.display()))?;
    let limit = MAX_EXPANDED;
    hash_stream(f, limit)
}
fn manifest_entry(rel: &str) -> bool { rel == MANIFEST }
fn content_files(root: &Path) -> Result<Vec<(String,u64)>> {
    let files = model::list_files(root);
    if files.len() > MAX_FILES { bail!("This model has too many files for one ZIP operation."); }
    for (rel, _) in &files { safe(root, rel)?; }
    Ok(files)
}
fn existing_case_conflict(root: &Path, rel: &str) -> Result<bool> {
    // Walk each component, rejecting Windows case-fold collisions on every OS.
    let mut at = root.to_path_buf();
    for component in rel.split('/') {
        if at.is_dir() {
            for item in fs::read_dir(&at)? {
                let name = item?.file_name().to_string_lossy().to_string();
                if name.eq_ignore_ascii_case(component) && name != component { return Ok(true); }
            }
        }
        at.push(component);
    }
    Ok(false)
}
fn check_outputs(root: &Path, paths: &[String]) -> Result<()> {
    let mut keys = HashSet::new();
    let sources: HashSet<String> = paths.iter().map(|s| s.to_lowercase()).collect();
    for rel in paths {
        if !keys.insert(rel.to_lowercase()) || rel == model::SIDECAR || rel.starts_with("_thumbs/")
            || rel == MANIFEST || rel.starts_with("_library/") {
            bail!("The ZIP has conflicting or reserved output paths: {rel}");
        }
        let to = safe(root, rel)?;
        if to.exists() || existing_case_conflict(root, rel)? {
            bail!("The destination is taken (including case-only names): {rel}. Choose another ZIP or destination.");
        }
        for (i,_) in rel.char_indices().filter(|(_,c)| *c == '/') {
            if sources.contains(&rel[..i].to_lowercase()) {
                bail!("The ZIP contains both a file and its parent folder: {rel}");
            }
        }
    }
    Ok(())
}
fn zip_entries(path: &Path) -> Result<Vec<Value>> {
    let mut z = zip::ZipArchive::new(File::open(path)?).context("This is not a readable ZIP archive.")?;
    if z.len() > MAX_FILES + 500 { bail!("The ZIP has too many entries."); }
    let (mut files, mut total) = (vec![], 0u64);
    let mut names = HashSet::new();
    let mut components = std::collections::HashMap::<String,String>::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).context("The ZIP contains an unsupported or encrypted entry.")?;
        let name = e.name().to_string();
        let rel = name.trim_end_matches('/');
        archive::safe_entry(rel)?;
        if e.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000) {
            bail!("The ZIP contains a symbolic link; links cannot be extracted.");
        }
        if !names.insert(rel.to_lowercase()) { bail!("ZIP entries collide (including letter case): {rel}"); }
        let mut prefix = String::new();
        for part in rel.split('/') {
            if !prefix.is_empty() { prefix.push('/'); }
            prefix.push_str(part);
            if let Some(old) = components.insert(prefix.to_lowercase(), prefix.clone()) {
                if old != prefix { bail!("ZIP paths differ only by case: {old} and {prefix}"); }
            }
        }
        if e.is_dir() { continue; }
        // This explanatory manifest does not become a second live model.json.
        if manifest_entry(rel) { continue; }
        total = total.checked_add(e.size()).ok_or_else(|| anyhow!("ZIP expanded size overflow."))?;
        if total > MAX_EXPANDED || files.len() >= MAX_FILES { bail!("The ZIP is too large to extract safely."); }
        let (sha, size) = hash_stream(&mut e, MAX_EXPANDED)?;
        if size != e.size() { bail!("The ZIP's advertised size differs from its contents: {rel}"); }
        files.push(json!({"path":rel,"size":size,"sha256":sha}));
    }
    Ok(files)
}
pub fn plan(lib: &Library, ix: &Index, args: &Value) -> Result<Value> {
    let model_id = field(args, "id");
    let m = ix.get(model_id).ok_or_else(|| anyhow!("Read the library again: that model is gone."))?;
    let root = lib.resolve(m.rel())?;
    if !root.is_dir() { bail!("The model directory is unavailable."); }
    approved_root(lib,&root)?;
    let action = field(args, "action");
    if !matches!(action, "compress" | "extract") { bail!("Choose ZIP compression or extraction."); }
    let archive_file = field(args, "file");
    let file = if archive_file.is_empty() && action == "compress" { "Model.zip" } else { archive_file };
    if !file.to_ascii_lowercase().ends_with(".zip") { bail!("Choose a .zip archive."); }
    if action == "compress" && file.contains('/') { bail!("Create the ZIP in the model's top folder."); }
    let at = safe(&root, file)?;
    let mut inputs = vec![];
    let mut output_paths = vec![];
    let source_sha;
    let bytes;
    if action == "compress" {
        check_outputs(&root, &[file.to_string()])?;
        let mut total = 0u64;
        for (rel, _) in content_files(&root)? {
            if rel == MANIFEST { bail!("The archive manifest path is reserved; rename that file first."); }
            let (sha, size) = file_hash(&safe(&root, &rel)?)?;
            total = total.checked_add(size).ok_or_else(|| anyhow!("Model size overflow."))?;
            if total > MAX_EXPANDED { bail!("This model is too large for one ZIP operation."); }
            inputs.push(json!({"path":rel,"sha256":sha,"size":size}));
        }
        if inputs.is_empty() { bail!("This model has no content files to compress."); }
        source_sha = String::new();
        bytes = total;
        output_paths.push(file.to_string());
    } else {
        if !at.is_file() { bail!("That archive is no longer in the model."); }
        (source_sha, _) = file_hash(&at)?;
        inputs = zip_entries(&at)?;
        if inputs.is_empty() { bail!("There are no files to extract from the ZIP."); }
        output_paths = inputs.iter().map(|x| field(x, "path").to_string()).collect();
        check_outputs(&root, &output_paths)?;
        bytes = inputs.iter().map(|x| x["size"].as_u64().unwrap_or(0)).sum();
    }
    Ok(json!({
        "action": action, "model": m.rel(), "model_id": model_id,
        "name": m.v["name"], "file": file, "source_sha256": source_sha,
        "files": inputs, "output_paths": output_paths,
        "files_count": inputs.len(), "bytes": bytes,
        "required_bytes": if action == "compress" { bytes + (bytes / 10) + 1024 * 1024 } else { bytes },
        "metadata": "Content files only; model.json and generated previews remain outside the ZIP. A separate archive-manifest.json records the original model and file checksums."
    }))
}
fn validate(lib: &Library, plan: &Value) -> Result<PathBuf> {
    let root = lib.resolve(field(plan,"model"))?;
    if !root.is_dir() { bail!("The source model moved. Review the plan again."); }
    approved_root(lib,&root)?;
    let side = model::read_sidecar(&root);
    if side["id"].as_str().is_some_and(|id| id != field(plan,"model_id")) {
        bail!("The model identity changed. Review the plan again.");
    }
    let outputs: Vec<String> = plan["output_paths"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
    check_outputs(&root, &outputs)?;
    if plan["action"] == "compress" {
        let current: Vec<String> = content_files(&root)?.into_iter().map(|(rel,_)|rel).collect();
        let proposed: Vec<String> = plan["files"].as_array().into_iter().flatten().map(|x|field(x,"path").to_string()).collect();
        if current != proposed { bail!("Model files changed since the preview. Review the plan again."); }
        for f in plan["files"].as_array().into_iter().flatten() {
            let (sha, size) = file_hash(&safe(&root,field(f,"path"))?)?;
            if sha != field(f,"sha256") || size != f["size"].as_u64().unwrap_or(0) {
                bail!("Source files changed since the preview. Review the plan again.");
            }
        }
    } else if file_hash(&safe(&root,field(plan,"file"))?)?.0 != field(plan,"source_sha256") {
        bail!("The source ZIP changed since the preview. Review the plan again.");
    }
    Ok(root)
}
fn make_zip(root: &Path, plan: &Value, to: &Path, cancel: &AtomicBool, progress: &dyn Fn(usize,usize,&str)) -> Result<()> {
    let inputs = plan["files"].as_array().unwrap();
    let mut zip = zip::ZipWriter::new(File::create(to)?);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (i,f) in inputs.iter().enumerate() {
        stopped(cancel)?;
        let rel = field(f,"path");
        progress(i, inputs.len(), rel);
        zip.start_file(rel, opt)?;
        let mut src = File::open(safe(root,rel)?)?;
        std::io::copy(&mut src, &mut zip)?;
    }
    zip.start_file(MANIFEST, opt)?;
    let description = json!({"format":"model-library-content-zip-v1","model_id":plan["model_id"],"name":plan["name"],"files":plan["files"]});
    zip.write_all(serde_json::to_string_pretty(&description)?.as_bytes())?;
    zip.finish()?.sync_all()?;
    // Re-open and hash every uncompressed entry; CRC/ZIP success is insufficient.
    let entries = zip_entries(to)?;
    if entries != *inputs { bail!("ZIP verification failed: expanded bytes differ from the sources."); }
    Ok(())
}
fn unpack_zip(root: &Path, plan: &Value, stage: &Path, cancel: &AtomicBool, progress: &dyn Fn(usize,usize,&str)) -> Result<()> {
    let mut z = zip::ZipArchive::new(File::open(safe(root,field(plan,"file"))?)?)?;
    let entries = plan["files"].as_array().unwrap();
    for (i,f) in entries.iter().enumerate() {
        stopped(cancel)?;
        let rel = field(f,"path");
        progress(i,entries.len(),rel);
        let dest = safe(stage,rel)?;
        fs::create_dir_all(dest.parent().unwrap())?;
        let mut entry = z.by_name(rel)?;
        let mut out = File::create(&dest)?;
        // Bound actual expanded output even if archive metadata is dishonest.
        let limit = f["size"].as_u64().unwrap_or(0);
        let written = std::io::copy(&mut entry.take(limit + 1), &mut out)?;
        if written != limit { bail!("The ZIP entry expanded to an unexpected size: {rel}"); }
        out.sync_all()?;
        let (hash,len)=file_hash(&dest)?;
        if hash != field(f,"sha256") || len != f["size"].as_u64().unwrap_or(0) {
            bail!("Archive verification failed for {rel}. Nothing has been removed.");
        }
    }
    Ok(())
}
pub fn execute(lib: &Library, plan: &Value, remove_sources: bool, cancel: &AtomicBool, progress: &dyn Fn(usize,usize,&str)) -> Result<Value> {
    lib.writable()?;
    stopped(cancel)?;
    let root = validate(lib,plan)?;
    let id = relayout::new_id(lib)?;
    let stage_name = format!(".archive-stage-{id}");
    let stage = root.join(&stage_name);
    fs::create_dir(&stage)?;
    let mut j = json!({
        "kind":"archive-op","id":id,"label":if plan["action"] == "compress" {"Compressed model to ZIP"} else {"Extracted ZIP into model"},
        "state":"running","created":crate::library::now(),"action":plan["action"],
        "model":plan["model"],"model_id":plan["model_id"],"file":plan["file"],
        "plan":plan,"remove_sources":remove_sources,"stage":stage_name,
        "moves":[{"from":plan["model"],"to":plan["model"]}],"outputs":[]
    });
    relayout::write(lib,&j)?;
    let result = (|| -> Result<Value> {
        if plan["action"] == "compress" {
            make_zip(&root,plan,&stage.join(field(plan,"file")),cancel,progress)?;
            let (sha,len) = file_hash(&stage.join(field(plan,"file")))?;
            j["outputs"] = json!([{"path":plan["file"],"sha256":sha,"size":len}]);
        } else {
            unpack_zip(&root,plan,&stage,cancel,progress)?;
            j["outputs"] = plan["files"].clone();
        }
        // A changed source invalidates cleanup AND publication, even if the
        // staged archive or extracted files are internally consistent.
        validate(lib,plan)?;
        stopped(cancel)?;
        relayout::write(lib,&j)?; // record hashes BEFORE publishing any file
        for f in j["outputs"].as_array().unwrap() {
            let rel = field(f,"path");
            let to = safe(&root,rel)?;
            if to.exists() { bail!("Output destination changed during the operation: {rel}"); }
            fs::create_dir_all(to.parent().unwrap())?;
            // A hard link publishes the staged, verified file without permitting
            // rename() to overwrite a file concurrently created on Unix.
            // The staging path stays available until the whole publish finishes.
            fs::hard_link(safe(&stage,rel)?,&to)
                .with_context(|| format!("Cannot safely publish {rel} without overwriting"))?;
        }
        let _ = fs::remove_dir_all(&stage);
        // Only verified bytes may replace the originals; verify published
        // outputs again before taking any input out of the live model.
        for f in j["outputs"].as_array().unwrap() {
            let rel = field(f,"path");
            let (hash,len) = file_hash(&safe(&root,rel)?)?;
            if hash != field(f,"sha256") || len != f["size"].as_u64().unwrap_or(0) {
                bail!("Published archive bytes changed. Original files were kept.");
            }
        }
        if remove_sources {
            validate_sources(&root,plan)?;
            let recovery = relayout::kept_dir(lib,&id).join("sources");
            let sources: Vec<String> = if plan["action"] == "compress" {
                plan["files"].as_array().unwrap().iter().map(|x|field(x,"path").to_string()).collect()
            } else { vec![field(plan,"file").to_string()] };
            for rel in sources {
                let original = safe(&root,&rel)?;
                let keep = safe(&recovery,&rel)?;
                fs::create_dir_all(keep.parent().unwrap())?;
                fs::rename(&original,&keep)?;
                prune(&root,original.parent());
            }
        }
        j["state"] = json!("done");
        j["finished"] = json!(crate::library::now());
        relayout::write(lib,&j)?;
        Ok(json!({"journal":id,"refresh":[plan["model_id"]],"verified":true,"removed_sources":remove_sources,"outputs":j["outputs"]}))
    })();
    if let Err(e) = &result {
        j["state"] = json!("interrupted");
        j["error"] = json!(format!("{e:#}"));
        let _ = relayout::write(lib,&j);
    }
    result
}
/// Separate, opt-in cleanup after a *completed* operation. This is a second
/// user decision: first publish and verify, then review/remove exact originals.
pub fn cleanup(lib: &Library, id: &str, cancel: &AtomicBool) -> Result<Value> {
    lib.writable()?;
    stopped(cancel)?;
    let mut j = relayout::read(lib,id)?;
    if j["kind"] != "archive-op" || j["state"] != "done" {
        bail!("Finish and verify the ZIP operation before offering source cleanup.");
    }
    if j["remove_sources"] == true { bail!("These sources were already put in recovery."); }
    let root = lib.resolve(field(&j,"model"))?;
    approved_root(lib,&root)?;
    let plan = &j["plan"];
    let outputs = j["outputs"].as_array().ok_or_else(|| anyhow!("The ZIP verification record is missing."))?;
    for f in outputs {
        let path = safe(&root,field(f,"path"))?;
        let (hash,len) = file_hash(&path)?;
        if hash != field(f,"sha256") || len != f["size"].as_u64().unwrap_or(0) {
            bail!("A published output changed: {}. Source cleanup is unavailable.",field(f,"path"));
        }
    }
    validate_sources(&root,plan)?;
    let keep = relayout::kept_dir(lib,id).join("sources");
    let sources: Vec<String> = if plan["action"] == "compress" {
        plan["files"].as_array().into_iter().flatten().map(|x|field(x,"path").to_string()).collect()
    } else { vec![field(plan,"file").to_string()] };
    // Verify recovery has no conflicting pre-existing files before moving any.
    for rel in &sources {
        stopped(cancel)?;
        if safe(&keep,rel)?.exists() { bail!("Recovery already contains {rel}. Resolve it before cleanup."); }
    }
    for rel in &sources {
        stopped(cancel)?;
        let from = safe(&root,rel)?;
        let to = safe(&keep,rel)?;
        fs::create_dir_all(to.parent().unwrap())?;
        fs::rename(&from,&to).with_context(|| format!("Could not move {rel} into recovery"))?;
        prune(&root,from.parent());
    }
    j["remove_sources"] = json!(true);
    j["cleaned_at"] = json!(crate::library::now());
    relayout::write(lib,&j)?;
    Ok(json!({"journal":id,"removed_sources":sources.len(),"refresh":[j["model_id"]]}))
}
fn validate_sources(root: &Path,plan: &Value) -> Result<()> {
    if plan["action"] == "compress" {
        for f in plan["files"].as_array().into_iter().flatten() {
            let (sha,len) = file_hash(&safe(root,field(f,"path"))?)?;
            if sha != field(f,"sha256") || len != f["size"].as_u64().unwrap_or(0) {
                bail!("An original changed after verification, so cleanup was cancelled.");
            }
        }
    } else if file_hash(&safe(root,field(plan,"file"))?)?.0 != field(plan,"source_sha256") {
        bail!("The source archive changed, so cleanup was cancelled.");
    }
    Ok(())
}
fn prune(root: &Path, from: Option<&Path>) {
    let mut p = from.map(Path::to_path_buf);
    while let Some(dir) = p {
        if dir == root || !dir.starts_with(root) { break; }
        if fs::remove_dir(&dir).is_err() { break; }
        p = dir.parent().map(Path::to_path_buf);
    }
}
pub fn undo(lib: &Library, mut journal: Value) -> Result<Value> {
    lib.writable()?;
    if journal["kind"] != "archive-op" { bail!("This is not a ZIP operation."); }
    if journal["state"] == "undone" { bail!("This ZIP operation was already undone."); }
    let root = lib.resolve(field(&journal,"model"))?;
    approved_root(lib,&root)?;
    let id = field(&journal,"id").to_string();
    let outputs = journal["outputs"].as_array().cloned().unwrap_or_default();
    // Validate the entire undo before moving anything: no changed output is
    // deleted, and no new input can be overwritten by a recovery copy.
    for f in &outputs {
        let path = safe(&root,field(f,"path"))?;
        if path.exists() && (file_hash(&path)?.0 != field(f,"sha256") || file_hash(&path)?.1 != f["size"].as_u64().unwrap_or(0)) {
            bail!("{} changed since the ZIP operation. Move it aside or restore from the journal manually.",field(f,"path"));
        }
    }
    let recovery = relayout::kept_dir(lib,&id).join("sources");
    let plan = &journal["plan"];
    let sources: Vec<String> = if plan["action"] == "compress" {
        plan["files"].as_array().into_iter().flatten().map(|x|field(x,"path").to_string()).collect()
    } else { vec![field(plan,"file").to_string()] };
    for rel in &sources {
        let saved = safe(&recovery,rel)?;
        if saved.exists() {
            let target = safe(&root,rel)?;
            if target.exists() { bail!("Cannot restore {rel}: a new file now occupies its path."); }
            let expected = if plan["action"] == "compress" {
                plan["files"].as_array().unwrap().iter().find(|f|field(f,"path")==rel).map(|f|field(f,"sha256")).unwrap_or("")
            } else { field(plan,"source_sha256") };
            if file_hash(&saved)?.0 != expected { bail!("Recovery file {rel} failed verification."); }
        }
    }
    journal["state"] = json!("interrupted");
    journal["direction"] = json!("undo");
    relayout::write(lib,&journal)?;
    for rel in &sources {
        let saved = safe(&recovery,rel)?;
        if saved.exists() {
            let dst = safe(&root,rel)?;
            fs::create_dir_all(dst.parent().unwrap())?;
            fs::rename(saved,dst)?;
        }
    }
    for f in &outputs {
        let dest = safe(&root,field(f,"path"))?;
        if dest.exists() {
            fs::remove_file(&dest)?;
            prune(&root,dest.parent());
        }
    }
    let stage = root.join(field(&journal,"stage"));
    if stage.file_name().and_then(|x|x.to_str()).is_some_and(|x|x.starts_with(".archive-stage-")) {
        let _ = fs::remove_dir_all(stage);
    }
    journal["state"] = json!("undone");
    journal["error"] = Value::Null;
    relayout::write(lib,&journal)?;
    Ok(json!({"undone":true,"refresh":[journal["model_id"]],"id":id}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::temp_dir;
    fn setup(label: &str) -> (Library,Index,String) {
        let root = temp_dir(label);
        let lib = Library::open(&root).unwrap();
        let dir = root.join("Unsorted/Kit");
        fs::create_dir_all(dir.join("Parts")).unwrap();
        fs::write(dir.join("Parts/body.stl"),b"mesh bytes").unwrap();
        fs::write(dir.join("README.pdf"),b"%PDF-1.4\nexample-document").unwrap();
        fs::write(dir.join("weird.unknown"),b"unknown file kept").unwrap();
        fs::write(dir.join(model::SIDECAR),b"{\"id\":\"zip-source-kit\",\"name\":\"Kit\",\"custom\":42}\n").unwrap();
        let ix = Index::build(&lib,None,true);
        let id = ix.models[0].id().to_string();
        (lib,ix,id)
    }
    #[test]
    fn verified_compress_keep_identity_and_hashes() {
        let (lib,ix,id) = setup("archive-cycle");
        let root=lib.root().join("Unsorted/Kit");
        let before: Vec<(String,String)> = model::list_files(&root).iter().map(|(n,_)|(n.clone(),file_hash(&root.join(n)).unwrap().0)).collect();
        let p=plan(&lib,&ix,&json!({"id":id,"action":"compress","file":"Kit.zip"})).unwrap();
        let r=execute(&lib,&p,false,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(r["verified"].as_bool().unwrap());
        assert!(root.join("Kit.zip").is_file());
        assert_eq!(model::read_sidecar(&root)["id"],id);
        let ix=Index::build(&lib,None,true);
        assert!(plan(&lib,&ix,&json!({"id":id,"action":"extract","file":"Kit.zip"})).is_err());
        relayout::undo(&lib,field(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(!root.join("Kit.zip").exists());
        for (n,hash) in before { assert_eq!(file_hash(&root.join(n)).unwrap().0,hash); }
    }
    #[test]
    fn opt_in_cleanup_then_extract_and_undo() {
        let (lib,ix,id)=setup("archive-clean");
        let root=lib.root().join("Unsorted/Kit");
        let p=plan(&lib,&ix,&json!({"id":id,"action":"compress","file":"Kit.zip"})).unwrap();
        let r=execute(&lib,&p,false,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(root.join("Parts/body.stl").is_file());
        cleanup(&lib,field(&r,"journal"),&AtomicBool::new(false)).unwrap();
        assert!(!root.join("Parts/body.stl").exists());
        assert!(root.join(model::SIDECAR).is_file());
        let ix=Index::build(&lib,None,true);
        let ep=plan(&lib,&ix,&json!({"id":id,"action":"extract","file":"Kit.zip"})).unwrap();
        let er=execute(&lib,&ep,false,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(root.join("Kit.zip").is_file());
        cleanup(&lib,field(&er,"journal"),&AtomicBool::new(false)).unwrap();
        assert!(!root.join("Kit.zip").exists());
        assert_eq!(fs::read(root.join("Parts/body.stl")).unwrap(),b"mesh bytes");
        relayout::undo(&lib,field(&er,"journal"),&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(root.join("Kit.zip").exists());
        assert!(!root.join("Parts/body.stl").exists());
        relayout::undo(&lib,field(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert_eq!(fs::read(root.join("Parts/body.stl")).unwrap(),b"mesh bytes");
        assert!(!root.join("Kit.zip").exists());
    }
    #[test]
    fn malicious_paths_and_corrupt_zip_preserve_sources() {
        let (lib,_,id)=setup("archive-unsafe");
        let root=lib.root().join("Unsorted/Kit");
        let add_zip=|name:&str, entries:&[(&str,&[u8])]| {
            let mut z=zip::ZipWriter::new(File::create(root.join(name)).unwrap());
            for (entry,data) in entries { z.start_file(*entry,zip::write::SimpleFileOptions::default()).unwrap(); z.write_all(data).unwrap(); }
            z.finish().unwrap();
        };
        add_zip("unsafe.zip",&[("../escape.txt",b"do not escape")]);
        let ix=Index::build(&lib,None,true);
        assert!(plan(&lib,&ix,&json!({"id":id,"action":"extract","file":"unsafe.zip"})).is_err());
        add_zip("collision.zip",&[("A.txt",b"a"),("a.TXT",b"b")]);
        assert!(plan(&lib,&ix,&json!({"id":id,"action":"extract","file":"collision.zip"})).is_err());
        fs::write(root.join("broken.zip"),b"invalid ZIP").unwrap();
        assert!(plan(&lib,&ix,&json!({"id":id,"action":"extract","file":"broken.zip"})).is_err());
        assert!(root.join("Parts/body.stl").exists());
    }
    #[test]
    fn cancel_and_undo_refuse_modified_output() {
        let (lib,ix,id)=setup("archive-cancel");
        let root=lib.root().join("Unsorted/Kit");
        let p=plan(&lib,&ix,&json!({"id":id,"action":"compress","file":"Kit.zip"})).unwrap();
        let stop=AtomicBool::new(false);
        assert!(execute(&lib,&p,true,&stop,&|_,_,_|{stop.store(true,Ordering::Relaxed)}).is_err());
        assert!(root.join("Parts/body.stl").exists());
        let r=execute(&lib,&p,false,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        fs::write(root.join("Kit.zip"),b"changed externally").unwrap();
        assert!(relayout::undo(&lib,field(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).is_err());
        assert_eq!(fs::read(root.join("Kit.zip")).unwrap(),b"changed externally");
    }
}
