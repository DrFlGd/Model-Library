// The 3D viewer (from Grid Workshop's): a model in a Z-up scene on a floor grid
// in 10 mm squares, orbit with the mouse, three views, edges on or off.
// Models arrive as binary STL from the core (it reads STL, OBJ and 3MF).
import * as THREE from "three";
import { OrbitControls } from "./vendor/three/OrbitControls.js";
import { STLLoader } from "./vendor/three/STLLoader.js";

const STEP = 10; // mm

export class Viewer {
  constructor(canvas) {
    this.canvas = canvas;
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(35, 1, 0.5, 20000);
    this.camera.up.set(0, 0, 1);
    this.camera.position.set(160, -220, 170);
    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.screenSpacePanning = true;

    this.scene.add(new THREE.HemisphereLight(0xffffff, 0x8a96a3, 1.6));
    const key = new THREE.DirectionalLight(0xffffff, 2.0);
    key.position.set(1, -1.5, 2.5);
    this.scene.add(key);
    const rim = new THREE.DirectionalLight(0xffffff, 0.7);
    rim.position.set(-2, 2, 1);
    this.scene.add(rim);

    this.material = new THREE.MeshStandardMaterial({ color: 0xf2b705, roughness: 0.55, metalness: 0.02, side: THREE.DoubleSide });
    this.edgeMaterial = new THREE.LineBasicMaterial({ color: 0x18222d, transparent: true, opacity: 0.55 });
    this.mesh = null;
    this.edges = null;
    this.gridGroup = new THREE.Group();
    this.scene.add(this.gridGroup);
    this.showEdges = false;
    this.bbox = null;
    this.dark = false;
    this._buildGrid(new THREE.Box3(new THREE.Vector3(-30, -30, 0), new THREE.Vector3(30, 30, 0)));

    // draw only when something changed
    this._raf = 0;
    const tick = () => {
      this._raf = 0;
      this.controls.update();
      this.renderer.render(this.scene, this.camera);
    };
    this.redraw = () => { if (!this._raf) this._raf = requestAnimationFrame(tick); };
    this.controls.addEventListener("change", this.redraw);
    this.controls.addEventListener("start", this.redraw);
    this._observer = new ResizeObserver(() => this.resize());
    this._observer.observe(canvas.parentElement);
    this.resize();
  }

  resize() {
    const el = this.canvas.parentElement;
    const w = el.clientWidth, h = el.clientHeight;
    if (!w || !h) return;
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.redraw();
  }

  setEdges(on) {
    this.showEdges = on;
    if (this.edges) this.edges.visible = on;
    this.redraw();
  }

  setTheme(dark) {
    if (this.dark === !!dark) return;
    this.dark = !!dark;
    this._buildGrid(this.bbox || new THREE.Box3(new THREE.Vector3(-30, -30, 0), new THREE.Vector3(30, 30, 0)));
    this.redraw();
  }

  /** Show a binary STL (ArrayBuffer). Returns its size and triangle count. */
  show(buf) {
    const geom = new STLLoader().parse(buf);
    geom.computeBoundingBox();
    const bb = geom.boundingBox;
    // rest it on the floor, centred
    geom.translate(-(bb.min.x + bb.max.x) / 2, -(bb.min.y + bb.max.y) / 2, -bb.min.z);
    geom.computeVertexNormals();
    geom.computeBoundingBox();
    this.clear();
    this.mesh = new THREE.Mesh(geom, this.material);
    this.scene.add(this.mesh);
    const triangles = geom.attributes.position.count / 3;
    if (triangles < 2000000) {
      this.edges = new THREE.LineSegments(new THREE.EdgesGeometry(geom, 30), this.edgeMaterial);
      this.edges.visible = this.showEdges;
      this.scene.add(this.edges);
    }
    this.bbox = geom.boundingBox.clone();
    this._buildGrid(this.bbox);
    this.view("iso");
    const size = this.bbox.getSize(new THREE.Vector3());
    return { x: size.x, y: size.y, z: size.z, triangles };
  }

  clear() {
    for (const o of [this.mesh, this.edges]) {
      if (o) { this.scene.remove(o); o.geometry.dispose(); }
    }
    this.mesh = this.edges = null;
    this.redraw();
  }

  /** The current view as a PNG data URL (for a cover picture). */
  snapshot() {
    this.controls.update();
    this.gridGroup.visible = false;
    this.renderer.render(this.scene, this.camera);
    const url = this.canvas.toDataURL("image/png");
    this.gridGroup.visible = true;
    this.redraw();
    return url;
  }

  dispose() {
    cancelAnimationFrame(this._raf);
    this._observer.disconnect();
    this.clear();
    this.controls.dispose();
    this.renderer.dispose();
  }

  _buildGrid(bb) {
    for (const c of [...this.gridGroup.children]) { this.gridGroup.remove(c); c.geometry?.dispose(); }
    const span = Math.max(bb.max.x - bb.min.x, bb.max.y - bb.min.y, 40);
    const step = span > 600 ? STEP * 10 : STEP;
    const half = Math.ceil((span * 0.75) / step) * step;
    const pts = [];
    for (let v = -half; v <= half + 0.01; v += step) pts.push(v, -half, 0, v, half, 0, -half, v, 0, half, v, 0);
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
    const lines = new THREE.LineSegments(g, new THREE.LineBasicMaterial({ color: this.dark ? 0x5b6976 : 0x8693a0, transparent: true, opacity: this.dark ? 0.6 : 0.45 }));
    const plate = new THREE.Mesh(new THREE.PlaneGeometry(half * 2, half * 2),
      new THREE.MeshBasicMaterial({ color: this.dark ? 0x26313c : 0xeef1f4, transparent: true, opacity: this.dark ? 0.85 : 0.7, depthWrite: false }));
    plate.position.z = -0.05;
    this.gridGroup.add(plate, lines);
  }

  view(name) {
    const bb = this.bbox || new THREE.Box3(new THREE.Vector3(-30, -30, 0), new THREE.Vector3(30, 30, 30));
    const size = bb.getSize(new THREE.Vector3());
    const center = bb.getCenter(new THREE.Vector3());
    const radius = Math.max(size.length() / 2, 10);
    const dist = (radius / Math.sin(THREE.MathUtils.degToRad(this.camera.fov / 2))) * 1.15;
    const dirs = { iso: new THREE.Vector3(0.62, -0.95, 0.75), top: new THREE.Vector3(0, -0.0001, 1), front: new THREE.Vector3(0, -1, 0.08) };
    const dir = (dirs[name] || dirs.iso).normalize();
    this.controls.target.copy(center);
    this.camera.position.copy(center).addScaledVector(dir, dist);
    this.camera.near = Math.max(dist / 200, 0.1);
    this.camera.far = dist * 50;
    this.camera.updateProjectionMatrix();
    this.controls.update();
    this.redraw();
  }
}
