// node combine.mjs <out.glb> : the mannequin mesh + every clip in ../assets/anims as named animations in ONE glb.
import { NodeIO } from '@gltf-transform/core';
import { ALL_EXTENSIONS } from '@gltf-transform/extensions';
import fs from 'fs';
const io = new NodeIO().registerExtensions(ALL_EXTENSIONS);
const base = await io.read('../assets/mannequin.glb'); const root = base.getRoot(); const buf = root.listBuffers()[0];
const byName = new Map(root.listNodes().map((n) => [n.getName(), n]));
let n = 0, skipped = 0;
for (const f of fs.readdirSync('../assets/anims').filter((x) => x.endsWith('.glb')).sort()) {
  const d = await io.read('../assets/anims/' + f); const a = d.getRoot().listAnimations()[0]; if (!a) continue;
  const anim = base.createAnimation(f.replace('.glb', ''));
  for (const ch of a.listChannels()) {
    const node = byName.get(ch.getTargetNode().getName()); if (!node) { skipped++; continue; }
    const s = ch.getSampler(), i = s.getInput(), o = s.getOutput();
    const inp = base.createAccessor().setType(i.getType()).setArray(i.getArray().slice()).setBuffer(buf);
    const out = base.createAccessor().setType(o.getType()).setArray(o.getArray().slice()).setBuffer(buf);
    const smp = base.createAnimationSampler().setInput(inp).setOutput(out).setInterpolation(s.getInterpolation());
    anim.addSampler(smp).addChannel(base.createAnimationChannel().setTargetNode(node).setTargetPath(ch.getTargetPath()).setSampler(smp));
  }
  n++;
}
await io.write(process.argv[2], base); console.log('animations', n, 'channels skipped', skipped);
