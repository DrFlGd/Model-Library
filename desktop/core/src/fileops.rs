//! Reviewed, SHA-256-verified file transfers into an existing model.
//! The journal retains recovery copies; destinations are published one file at a time.
use crate::{archive, extract, import, index::Index, library::Library, model, relayout, thumb};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::{collections::{BTreeMap, HashSet}, path::{Path,PathBuf}, sync::atomic::{AtomicBool,Ordering}};

fn s<'a>(v:&'a Value,k:&str)->&'a str {v[k].as_str().unwrap_or("")}
fn stop(c:&AtomicBool)->Result<()> {if c.load(Ordering::Relaxed) {bail!("Stopped.");} Ok(())}
fn checked_file(root:&Path, rel:&str)->Result<PathBuf> {
    let p=extract::checked(root,rel)?;
    if !p.is_file() {bail!("{rel} is no longer a file.");}
    Ok(p)
}
fn list_selected(root:&Path,rel:&str,out:&mut BTreeMap<String,Value>)->Result<()> {
    let p=if rel.is_empty() {root.to_path_buf()} else {extract::checked(root,rel)?};
    if p.is_dir() {
        for e in std::fs::read_dir(p)? {
            let e=e?;
            let name=e.file_name().to_string_lossy().to_string();
            let child=if rel.is_empty(){name}else{format!("{rel}/{name}")};
            if child==model::SIDECAR || child=="_thumbs" || child.starts_with("_thumbs/") {continue;}
            if name.starts_with('.') {continue;}
            list_selected(root,&child,out)?;
        }
    } else if p.is_file() {
        if rel==model::SIDECAR || rel.starts_with("_thumbs/") {bail!("Model details and previews stay with their model.");}
        out.insert(rel.to_string(),json!({"from":rel,"name":rel,"entry":null}));
    } else {bail!("{rel} is missing.");}
    Ok(())
}
fn existing_name(dir:&Path,name:&str)->Result<bool> {
    if !dir.exists(){return Ok(false);}
    if !dir.is_dir(){return Ok(true);}
    for e in std::fs::read_dir(dir)? {
        if e?.file_name().to_string_lossy().eq_ignore_ascii_case(name) {return Ok(true);}
    }
    Ok(false)
}
fn conflicts_with_disk(root:&Path,rel:&str)->Result<bool> {
    let mut at=root.to_path_buf();
    let comps:Vec<_>=extract::safe(rel)?.components().map(|v|v.as_os_str().to_string_lossy().into_owned()).collect();
    for (i,part) in comps.iter().enumerate() {
        if existing_name(&at,part)? {
            let chosen=at.join(part);
            if i==comps.len()-1 {return Ok(true);}
            if chosen.is_symlink() || !chosen.is_dir() {return Ok(true);}
            at=chosen;
        } else {
            // A parent differing only by case is not a safe path even on Linux.
            if at.exists() {
                for e in std::fs::read_dir(&at)? {
                    if e?.file_name().to_string_lossy().eq_ignore_ascii_case(part) {return Ok(true);}
                }
            }
            return Ok(false);
        }
    }
    Ok(false)
}
fn unique(root:&Path, rel:&str, used:&HashSet<String>)->Result<String> {
    let p=extract::safe(rel)?;
    let parent=p.parent().unwrap_or(Path::new(""));
    let file=p.file_name().ok_or_else(||anyhow!("Invalid filename."))?.to_string_lossy().to_string();
    let (stem,ext)=match file.rsplit_once('.') {Some((a,b)) if !a.is_empty()=>(a.to_string(),format!(".{b}")),_=>(file,String::new())};
    for i in 2..10000 {
        let candidate=parent.join(format!("{stem} ({i}){ext}")).to_string_lossy().replace('\\',"/");
        if !used.contains(&candidate.to_lowercase()) && !conflicts_with_disk(root,&candidate)? {return Ok(candidate);}
    }
    bail!("Too many conflicting filenames.")
}
fn checked_external(name:&str)->Result<PathBuf> {
    let p=PathBuf::from(name);
    if !p.is_absolute() || !p.is_file() {bail!("Choose existing files to add.");}
    let mut at=PathBuf::new();
    for comp in p.components() {
        at.push(comp);
        if std::fs::symlink_metadata(&at).is_ok_and(|m|m.file_type().is_symlink()) {
            bail!("Links cannot be added to a model.");
        }
    }
    Ok(p)
}
fn data_of(source:&Path,entry:Option<&str>)->Result<Vec<u8>> {
    if let Some(e)=entry {
        // ZIP entry extraction is intentionally bounded (agent C owns archive capabilities).
        archive::read(source,e,512*1024*1024)
    } else {Ok(std::fs::read(source)?)}
}
fn sha_bytes(bytes:&[u8])->String {
    use sha2::{Sha256,Digest};
    hex::encode(Sha256::digest(bytes))
}

pub fn plan(lib:&Library,ix:&Index,args:&Value)->Result<Value> {
    let kind=s(args,"kind");
    if !matches!(kind,"send"|"add") {bail!("Choose Send to another model or Add files.");}
    let target_id=s(args,"target");
    let target=ix.get(target_id).ok_or_else(||anyhow!("Choose a destination model."))?;
    let target_root=extract::checked(lib.root(),target.rel())?;
    let folder=s(args,"folder");
    let dest_folder=if folder.is_empty(){target_root.clone()}else{extract::checked(&target_root,folder)?};
    if !dest_folder.is_dir() {bail!("Choose the model root or an existing folder.");}
    let mode=if kind=="add" {"copy"} else {s(args,"mode")};
    if !matches!(mode,"move"|"copy") {bail!("Choose Move or Copy.");}
    let on_conflict=args["conflict"].as_str().unwrap_or("cancel");
    if !matches!(on_conflict,"cancel"|"skip"|"keep_both") {bail!("Choose Keep both, Skip or Cancel.");}
    let mut choices=BTreeMap::<String,Value>::new();
    let mut source_rel=String::new();
    let mut source_id=String::new();
    let mut source_root=PathBuf::new();
    if kind=="send" {
        source_id=s(args,"id").into();
        if source_id==target_id {bail!("Select another model, not the source.");}
        let m=ix.get(&source_id).ok_or_else(||anyhow!("The source model is gone."))?;
        source_rel=m.rel().to_string();
        source_root=extract::checked(lib.root(),&source_rel)?;
        for f in args["files"].as_array().into_iter().flatten() {
            let rel=f.as_str().ok_or_else(||anyhow!("Invalid selection."))?;
            list_selected(&source_root,rel,&mut choices)?;
        }
        for e in args["entries"].as_array().into_iter().flatten() {
            let (file,entry)=(s(e,"file"),s(e,"entry"));
            let path=checked_file(&source_root,file)?;
            let prefix=entry.trim_end_matches('/');
            extract::safe(prefix)?;
            let mut matched=false;
            for item in archive::list(&path)? {
                let name=s(&item,"name");
                if name==prefix || name.starts_with(&format!("{prefix}/")) {
                    extract::safe(name)?;
                    matched=true;
                    choices.insert(format!("{file}!{name}"),json!({"from":file,"entry":name,"name":name}));
                }
            }
            if !matched {bail!("A selected ZIP entry is missing.");}
        }
        if choices.values().any(|v|v["entry"].is_string()) && mode=="move" {
            bail!("An individual ZIP entry can be copied, but not moved. The archive stays unchanged.");
        }
    } else {
        for path in args["sources"].as_array().into_iter().flatten() {
            let raw=path.as_str().ok_or_else(||anyhow!("Invalid file."))?;
            let p=checked_external(raw)?;
            let name=p.file_name().unwrap().to_string_lossy().to_string();
            choices.insert(raw.into(),json!({"from":raw,"entry":null,"name":name,"external":true}));
        }
    }
    if choices.is_empty(){bail!("Select at least one file.");}
    let mut files=vec![];
    let mut collisions=vec![];
    let mut seen=HashSet::new();
    let mut bytes=0u64;
    for (_,mut f) in choices {
        let rel=if folder.is_empty(){s(&f,"name").to_string()}else{format!("{folder}/{}",s(&f,"name"))};
        extract::safe(&rel)?;
        let dest_rel=rel.clone();
        let taken=seen.contains(&rel.to_lowercase()) || conflicts_with_disk(&target_root,&rel)?;
        if taken {collisions.push(json!({"source":f["from"],"path":rel}));}
        if taken && on_conflict=="skip" {continue;}
        let output=if taken && on_conflict=="keep_both" {unique(&target_root,&rel,&seen)?} else {dest_rel};
        if !seen.insert(output.to_lowercase()) {bail!("Two selected files have the same destination.");}
        let src=if f["external"]==true {checked_external(s(&f,"from"))?} else {checked_file(&source_root,s(&f,"from"))?};
        // Zip entry reads are capped; plain files use a streaming hash.
        let (hash,size)=if let Some(entry)=f["entry"].as_str() {
            let data=data_of(&src,Some(entry))?;
            (sha_bytes(&data),data.len() as u64)
        } else {(extract::hash(&src)?,std::fs::metadata(&src)?.len())};
        bytes=bytes.saturating_add(size);
        f["to"]=json!(output); f["sha256"]=json!(hash); f["bytes"]=json!(size);
        files.push(f);
    }
    Ok(json!({"kind":"file_transfer","operation_id":model::new_id(),"mode":mode,"source":source_rel,
        "source_id":source_id,"target":target.rel(),"target_id":target_id,"folder":folder,
        "files":files,"count":files.len(),"bytes":bytes,"conflicts":collisions,
        "conflict":on_conflict,"source_before":if kind=="send" {model::read_sidecar(&source_root)}else{json!(null)},
        "target_before":model::read_sidecar(&target_root),"reversible":true,
        "cleanup":if mode=="move" {"Remove only verified source files and empty source folders."}else{"Keep all originals."}}))
}
fn copy_checked(from:&Path,to:&Path,expected:&str)->Result<()> {
    std::fs::create_dir_all(to.parent().unwrap())?;
    let copy=(||->Result<()> {
        std::fs::copy(from,to)?;
        if extract::hash(to)? != expected {bail!("A copied file did not match its SHA-256 manifest.");}
        Ok(())
    })();
    if copy.is_err() {let _=std::fs::remove_file(to);}
    copy
}
fn publish(from:&Path,to:&Path,jid:&str,expected:&str)->Result<()> {
    if to.exists(){bail!("The destination changed. Review the operation again.");}
    std::fs::create_dir_all(to.parent().unwrap())?;
    let name=to.file_name().unwrap().to_string_lossy();
    let tmp=to.with_file_name(format!(".model-transfer-{jid}-{name}"));
    let result=(||->Result<()> {
        copy_checked(from,&tmp,expected)?;
        // An atomic no-overwrite publish: hard_link fails if another writer won the race.
        std::fs::hard_link(&tmp,to)?;
        if extract::hash(to)? != expected {bail!("The destination failed verification.");}
        Ok(())
    })();
    let _=std::fs::remove_file(tmp);
    result
}
fn dirs(root:&Path)->Result<Vec<String>> {
    fn visit(root:&Path,at:&Path,out:&mut Vec<String>)->Result<()> {
        for e in std::fs::read_dir(at)? {
            let e=e?;
            if e.file_type()?.is_symlink(){bail!("A model contains an unsafe link.");}
            if e.file_type()?.is_dir() {
                let p=e.path();
                out.push(p.strip_prefix(root)?.to_string_lossy().replace('\\',"/"));
                visit(root,&p,out)?;
            }
        }
        Ok(())
    }
    let mut out=vec![];visit(root,root,&mut out)?;Ok(out)
}
fn trim_empty(root:&Path,at:&Path) {
    let mut p=at.parent();
    while let Some(dir)=p {
        if dir==root || !dir.starts_with(root) || std::fs::remove_dir(dir).is_err(){break;}
        p=dir.parent();
    }
}
pub fn execute(lib:&Library,plan:&Value,cancel:&AtomicBool,progress:&dyn Fn(usize,usize,&str))->Result<Value> {
    lib.writable()?;
    let files=plan["files"].as_array().ok_or_else(||anyhow!("Invalid transfer plan."))?;
    if files.is_empty(){bail!("No files remain after resolving conflicts.");}
    if s(plan,"conflict")=="cancel" && plan["conflicts"].as_array().is_some_and(|a|!a.is_empty()) {
        bail!("Files already exist at the destination. Choose Keep both, Skip, or Cancel.");
    }
    let target=extract::checked(lib.root(),s(plan,"target"))?;
    let send=!s(plan,"source").is_empty();
    let source=if send {Some(extract::checked(lib.root(),s(plan,"source"))?)} else {None};
    if model::read_sidecar(&target)!=plan["target_before"] ||
        source.as_ref().is_some_and(|p|model::read_sidecar(p)!=plan["source_before"]) {
        bail!("A model's details changed since review. Review the transfer again.");
    }
    for f in files {
        stop(cancel)?;
        let dest=extract::checked(&target,s(f,"to"))?;
        if conflicts_with_disk(&target,s(f,"to"))? || dest.exists() {bail!("Destination changed. Review the transfer again.");}
        let from=if f["external"]==true{checked_external(s(f,"from"))?}
            else {checked_file(source.as_ref().ok_or_else(||anyhow!("Missing source model."))?,s(f,"from"))?};
        let actual=if let Some(entry)=f["entry"].as_str(){sha_bytes(&data_of(&from,Some(entry))?)}else{extract::hash(&from)?};
        if actual!=s(f,"sha256"){bail!("A selected file changed. Review the transfer again.");}
    }
    let jid=relayout::new_id(lib)?;
    let keep=relayout::kept_dir(lib,&jid);
    let stage=keep.join("staged");std::fs::create_dir_all(&stage)?;
    let mut j=plan.clone();
    j["id"]=json!(jid); j["kind"]=json!("file_transfer");
    j["label"]=json!(if send {format!("Sent files to {}",s(plan,"target").rsplit('/').next().unwrap_or("model"))} else {"Added files to model".into()});
    j["created"]=json!(crate::library::now()); j["state"]=json!("running");
    j["moves"]=json!([{"from":plan["source"],"to":plan["target"]}]);
    j["written"]=json!([]);j["removed"]=json!([]);j["retired"]=json!(false);
    if let Some(src)=&source {
        j["source_dirs"]=json!(dirs(src)?);
        let side=src.join(model::SIDECAR);
        if side.is_file(){std::fs::copy(&side,keep.join("source-sidecar"))?;}
        let thumbnail=src.join(thumb::THUMB);
        if thumbnail.is_file(){std::fs::copy(&thumbnail,keep.join("source-thumb"))?;}
    }
    relayout::write(lib,&j)?;
    let result=(||->Result<()> {
        for (i,f) in files.iter().enumerate() {
            stop(cancel)?;progress(i,files.len()*3,s(f,"from"));
            let from=if f["external"]==true {checked_external(s(f,"from"))?}
                else {checked_file(source.as_ref().unwrap(),s(f,"from"))?};
            let out=stage.join(i.to_string());
            if let Some(entry)=f["entry"].as_str() {
                std::fs::write(&out,data_of(&from,Some(entry))?)?;
            } else {copy_checked(&from,&out,s(f,"sha256"))?;}
            if extract::hash(&out)? != s(f,"sha256"){bail!("Staged files failed verification.");}
        }
        stop(cancel)?;
        for (i,f) in files.iter().enumerate() {
            progress(files.len()+i,files.len()*3,s(f,"to"));stop(cancel)?;
            if conflicts_with_disk(&target,s(f,"to"))? {bail!("A destination was taken during transfer.");}
            let dest=extract::checked(&target,s(f,"to"))?;
            j["publishing"]=json!(i);relayout::write(lib,&j)?;
            publish(&stage.join(i.to_string()),&dest,&jid,s(f,"sha256"))?;
            j["written"].as_array_mut().unwrap().push(json!(i));
            j["publishing"]=Value::Null;relayout::write(lib,&j)?;
        }
        if s(plan,"mode")=="move" {
            let src=source.as_ref().unwrap();
            // Verify ALL originals before removing the first; recovery copies are retained.
            for f in files {
                if f["entry"].is_string(){continue;}
                if extract::hash(&checked_file(src,s(f,"from"))?)?!=s(f,"sha256") {
                    bail!("The source changed during transfer. Sources will be restored.");
                }
            }
            for (i,f) in files.iter().enumerate() {
                progress(files.len()*2+i,files.len()*3,s(f,"from"));stop(cancel)?;
                if f["entry"].is_string(){continue;}
                j["removing"]=json!(i);relayout::write(lib,&j)?;
                let path=checked_file(src,s(f,"from"))?;
                std::fs::remove_file(&path)?;
                j["removed"].as_array_mut().unwrap().push(json!(i));
                j["removing"]=Value::Null;relayout::write(lib,&j)?;
                trim_empty(src,&path);
            }
            if model::list_files(src).is_empty() {
                j["retired"]=json!(true);j["retired_hashes"]=extract::hashes(src)?;
                relayout::write(lib,&j)?;
                std::fs::rename(src,keep.join("retired"))?;
            } else {
                let old=s(&plan["source_before"],"cover");
                if !old.is_empty() && !src.join(old).is_file() {
                    model::update(src,&json!({"cover":null}),&plan["source_before"])?;
                }
                let _=std::fs::remove_file(src.join(thumb::THUMB));
                let _=thumb::make(src);
            }
        }
        j["source_after"]=json!(source.as_ref().map(|p|{
            if j["retired"]==true {model::read_sidecar(&keep.join("retired"))}else{model::read_sidecar(p)}
        }));
        j["source_after_thumb"]=json!(source.as_ref().map(|p|{
            let dir=if j["retired"]==true {keep.join("retired")}else{p.clone()};
            extract::hash(&dir.join(thumb::THUMB)).ok()
        }));
        j["finished"]=json!(crate::library::now());j["state"]=json!("done");relayout::write(lib,&j)?;
        Ok(())
    })();
    if let Err(err)=result {
        match rollback(lib,&j,false) {
            Ok(())=>{j["state"]=json!("undone");relayout::write(lib,&j)?;}
            Err(e)=>{j["state"]=json!("stopped");j["error"]=json!(format!("{err}; recovery failed: {e}"));relayout::write(lib,&j)?;
                bail!("{err}; recovery failed: {e}. Restore from the journal before continuing.");}
        }
        return Err(err);
    }
    progress(files.len()*3,files.len()*3,"Complete");
    Ok(json!({"journal":jid,"count":files.len(),"refresh":[plan["source_id"],plan["target_id"]]}))
}
fn rollback(lib:&Library,j:&Value,strict:bool)->Result<()> {
    let files=j["files"].as_array().ok_or_else(||anyhow!("Invalid recovery record."))?;
    let keep=relayout::kept_dir(lib,s(j,"id"));
    let stage=keep.join("staged");
    let dest=extract::checked(lib.root(),s(j,"target"))?;
    let source=if s(j,"source").is_empty(){None}else{Some(extract::checked(lib.root(),s(j,"source"))?)};
    let retired=keep.join("retired");
    if strict {
        if model::read_sidecar(&dest)!=j["target_before"] {
            bail!("The destination model's details changed. Resolve this before Undo.");
        }
        if let Some(src)=&source {
            if j["retired"]==true {
                if src.exists() || !retired.is_dir() || extract::hashes(&retired)?!=j["retired_hashes"] {
                    bail!("The source model recovery changed or its former path was reused.");
                }
            } else if j["source_after"]!=json!(Some(model::read_sidecar(src))) ||
                j["source_after_thumb"]!=json!(Some(extract::hash(&src.join(thumb::THUMB)).ok())) {
                bail!("The source model has changed. Resolve this before Undo.");
            }
        }
    }
    // A stopped operation may have staged files without publishing them.
    // Never delete an unowned destination merely because its bytes happen to match.
    let published:HashSet<usize>=j["written"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|n|n as usize).collect();
    let maybe_publishing=j["publishing"].as_u64().map(|n|n as usize);
    if !strict && maybe_publishing.is_some() {
        bail!("Publishing was interrupted. Review the uncertain destination before recovery; no file was overwritten.");
    }
    // Check ALL restore and removal locations before making changes.
    for (i,f) in files.iter().enumerate() {
        if !strict && !published.contains(&i) { continue; }
        let output=extract::checked(&dest,s(f,"to"))?;
        if output.exists() && extract::hash(&output)?!=s(f,"sha256") {
            bail!("{} was changed after transfer; nothing was overwritten.",s(f,"to"));
        }
        if strict && !output.exists(){bail!("{} was removed after transfer; resolve before Undo.",s(f,"to"));}
        if s(j,"mode")=="move" && f["entry"].is_null() {
            let src=source.as_ref().unwrap();
            let from=extract::checked(src,s(f,"from"))?;
            if from.exists() && extract::hash(&from)?!=s(f,"sha256") {
                bail!("The original file path has been reused: {}.",s(f,"from"));
            }
            if !from.exists() && !stage.join(i.to_string()).is_file() {bail!("Recovery copy is missing.");}
        }
    }
    if j["retired"]==true && retired.is_dir() {
        let src=source.as_ref().unwrap();
        if src.exists(){bail!("The original model folder was reused.");}
        std::fs::create_dir_all(src.parent().unwrap())?;
        std::fs::rename(&retired,src)?;
    }
    if s(j,"mode")=="move" {
        let src=source.as_ref().unwrap();
        for (i,f) in files.iter().enumerate() {
            if f["entry"].is_string(){continue;}
            let old=extract::checked(src,s(f,"from"))?;
            if !old.exists(){copy_checked(&stage.join(i.to_string()),&old,s(f,"sha256"))?;}
        }
        if j["retired"]!=true {
            let before=&j["source_before"];
            let side=src.join(model::SIDECAR);
            if before.is_object() && !before.as_object().unwrap().is_empty() {
                if keep.join("source-sidecar").is_file(){std::fs::copy(keep.join("source-sidecar"),side)?;}
            } else if side.exists(){std::fs::remove_file(side)?;}
            let preview=src.join(thumb::THUMB);
            if keep.join("source-thumb").is_file() {
                std::fs::create_dir_all(preview.parent().unwrap())?;
                std::fs::copy(keep.join("source-thumb"),preview)?;
            } else if preview.exists() {std::fs::remove_file(preview)?;}
        }
        for d in j["source_dirs"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            std::fs::create_dir_all(extract::checked(src,d)?)?;
        }
    }
    // Only the files demonstrably published by this operation may be removed.
    for (i,f) in files.iter().enumerate() {
        if !strict && !published.contains(&i) { continue; }
        let output=extract::checked(&dest,s(f,"to"))?;
        if output.exists() {
            if extract::hash(&output)?!=s(f,"sha256") {bail!("The destination changed during recovery.");}
            std::fs::remove_file(&output)?;
            trim_empty(&dest,&output);
        }
    }
    Ok(())
}
pub fn undo(lib:&Library,mut j:Value)->Result<Value> {
    match rollback(lib,&j,true) {
        Ok(()) => {
            j["state"]=json!("undone");relayout::write(lib,&j)?;
            Ok(json!({"journal":j["id"],"state":"undone","failed":[],"refresh":[j["source_id"],j["target_id"]]}))
        }
        Err(e) => {
            j["state"]=json!("stopped");j["error"]=json!(e.to_string());relayout::write(lib,&j)?;Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::temp_dir;

    fn setup(tag: &str) -> (Library, String, String) {
        let root = temp_dir(tag);
        let lib = Library::open(&root).unwrap();
        for (name,id) in [("Source","source-1"),("Target","target-1")] {
            let dir = root.join("Unsorted").join(name);
            std::fs::create_dir_all(dir.join("Nested")).unwrap();
            std::fs::write(dir.join(model::SIDECAR),
                format!(r#"{{"id":"{id}","name":"{name}","custom":"unchanged"}}"#)).unwrap();
        }
        std::fs::write(root.join("Unsorted/Source/Nested/a.xyz"), b"unknown-extension").unwrap();
        std::fs::write(root.join("Unsorted/Source/Nested/b.pdf"), b"%PDF-1.5").unwrap();
        let ix = Index::build(&lib,None,true);
        let from=ix.models.iter().find(|m|m.v["name"]=="Source").unwrap().id().to_string();
        let to=ix.models.iter().find(|m|m.v["name"]=="Target").unwrap().id().to_string();
        (lib,from,to)
    }
    #[test]
    fn sends_nested_folder_and_undoes_without_changing_target_identity() {
        let (lib,from,to)=setup("send-nested");
        let src=lib.resolve("Unsorted/Source").unwrap();
        let dst=lib.resolve("Unsorted/Target").unwrap();
        let before=extract::hashes(&src).unwrap();
        let args=json!({"kind":"send","id":from,"target":to,"files":["Nested"],"mode":"move"});
        let ix=Index::build(&lib,None,true);
        let p=plan(&lib,&ix,&args).unwrap();
        assert_eq!(p["count"],2);
        let result=execute(&lib,&p,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(!src.exists(),"last-file Move should retire the source model");
        assert_eq!(model::read_sidecar(&dst)["id"],to);
        assert_eq!(std::fs::read(dst.join("Nested/a.xyz")).unwrap(),b"unknown-extension");
        relayout::undo(&lib,s(&result,"journal"),&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert_eq!(extract::hashes(&src).unwrap(),before);
        assert!(!dst.join("Nested/a.xyz").exists());
        assert_eq!(model::read_sidecar(&dst)["id"],to);
    }
    #[test]
    fn copies_and_resolves_case_insensitive_collisions() {
        let (lib,from,to)=setup("send-clash");
        let dst=lib.resolve("Unsorted/Target").unwrap();
        std::fs::write(dst.join("Nested/a.xyz"),"taken").unwrap();
        let ix=Index::build(&lib,None,true);
        let base=json!({"kind":"send","id":from,"target":to,"files":["Nested"],"mode":"copy"});
        let p=plan(&lib,&ix,&base).unwrap();
        assert_eq!(p["conflicts"].as_array().unwrap().len(),1);
        assert!(execute(&lib,&p,&AtomicBool::new(false),&|_,_,_|{}).is_err());
        let mut keep=base;
        keep["conflict"]=json!("keep_both");
        let p=plan(&lib,&ix,&keep).unwrap();
        let r=execute(&lib,&p,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert_eq!(std::fs::read(dst.join("Nested/a.xyz")).unwrap(),b"taken");
        assert!(dst.join("Nested/a (2).xyz").is_file());
        relayout::undo(&lib,s(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(!dst.join("Nested/a (2).xyz").exists());
        assert!(lib.resolve("Unsorted/Source/Nested/a.xyz").unwrap().exists());
    }
    #[test]
    fn adding_mixed_external_files_keeps_originals_and_can_undo() {
        let (lib,_,to)=setup("add-mixed");
        let external=temp_dir("add-external");
        std::fs::create_dir_all(&external).unwrap();
        let files=["image.png","video.mp4","document.pdf","other.7z"];
        let selected:Vec<String>=files.iter().map(|name| {
            let path=external.join(name);
            std::fs::write(&path,name.as_bytes()).unwrap();
            path.display().to_string()
        }).collect();
        let ix=Index::build(&lib,None,true);
        let p=plan(&lib,&ix,&json!({"kind":"add","target":to,"sources":selected,"folder":"Nested"})).unwrap();
        assert_eq!(p["count"],files.len());
        let r=execute(&lib,&p,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        for name in files {
            assert!(lib.resolve(&format!("Unsorted/Target/Nested/{name}")).unwrap().is_file());
            assert!(external.join(name).is_file());
        }
        relayout::undo(&lib,s(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        for name in files {
            assert!(!lib.resolve(&format!("Unsorted/Target/Nested/{name}")).unwrap().exists());
            assert!(external.join(name).is_file());
        }
    }
    #[test]
    fn destination_failure_and_modified_undo_cannot_lose_files() {
        let (lib,from,to)=setup("send-failure");
        let ix=Index::build(&lib,None,true);
        let args=json!({"kind":"send","id":from,"target":to,"files":["Nested"],"mode":"move"});
        let p=plan(&lib,&ix,&args).unwrap();
        let dst=lib.resolve("Unsorted/Target/Nested/a.xyz").unwrap();
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::write(&dst,"external-new-file").unwrap();
        assert!(execute(&lib,&p,&AtomicBool::new(false),&|_,_,_|{}).is_err());
        assert_eq!(std::fs::read(&dst).unwrap(),b"external-new-file");
        assert!(lib.resolve("Unsorted/Source/Nested/a.xyz").unwrap().exists());
        std::fs::remove_file(dst).unwrap();
        let r=execute(&lib,&p,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        let dst=lib.resolve("Unsorted/Target/Nested/a.xyz").unwrap();
        std::fs::write(&dst,"changed after send").unwrap();
        assert!(relayout::undo(&lib,s(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).is_err());
        assert_eq!(std::fs::read(&dst).unwrap(),b"changed after send");
    }
    #[test]
    fn moving_zip_entries_requires_archive_rewrite() {
        use std::io::Write;
        let (lib,from,to)=setup("send-zip");
        let src=lib.resolve("Unsorted/Source/more.zip").unwrap();
        let mut z=zip::ZipWriter::new(std::fs::File::create(&src).unwrap());
        z.start_file("Parts/item.xyz",zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"archive entry").unwrap();z.finish().unwrap();
        let ix=Index::build(&lib,None,true);
        let mut args=json!({"kind":"send","id":from,"target":to,"entries":[{"file":"more.zip","entry":"Parts/item.xyz"}],"mode":"move"});
        assert!(plan(&lib,&ix,&args).is_err());
        args["mode"]=json!("copy");
        let p=plan(&lib,&ix,&args).unwrap();
        let r=execute(&lib,&p,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert_eq!(std::fs::read(lib.resolve("Unsorted/Target/Parts/item.xyz").unwrap()).unwrap(),b"archive entry");
        assert!(src.is_file());
        relayout::undo(&lib,s(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(src.is_file());
    }
}
