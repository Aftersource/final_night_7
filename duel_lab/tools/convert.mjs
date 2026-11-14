// node convert.mjs base <in.glb> <out.glb>   |   node convert.mjs anim <in.glb> <out.glb> <name>
import { NodeIO } from '@gltf-transform/core';
import { ALL_EXTENSIONS } from '@gltf-transform/extensions';
import { textureCompress, prune, dedup } from '@gltf-transform/functions';
import sharp from 'sharp';
const [mode, src, out, name] = process.argv.slice(2);
const io = new NodeIO().registerExtensions(ALL_EXTENSIONS);
const doc = await io.read(src); const r = doc.getRoot();
if (mode === 'base') {
  await doc.transform(textureCompress({ encoder: sharp, targetFormat: 'webp', resize: [1024, 1024], quality: 85 }), dedup(), prune());
} else {
  for (const n of r.listNodes()) { n.setMesh(null); n.setSkin(null); }
  for (const m of r.listMeshes()) m.dispose();
  for (const s of r.listSkins()) s.dispose();
  for (const m of r.listMaterials()) m.dispose();
  for (const t of r.listTextures()) t.dispose();
  const a = r.listAnimations(); a.slice(1).forEach(x => x.dispose()); if (a[0]) a[0].setName(name);
  await doc.transform(prune({ keepLeaves: true }));
}
await io.write(out, doc);
const a = r.listAnimations()[0];
console.log(mode, out, a ? 'channels ' + a.listChannels().length : '');
