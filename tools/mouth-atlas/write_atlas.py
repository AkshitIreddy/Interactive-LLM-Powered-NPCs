"""Write the Mara schema-four oral-strip pack from procedural strips (matches prepare-current-pixel-mouth-pack.py layout)."""
import argparse, hashlib, json, sys
from pathlib import Path
import numpy as np, cv2
sys.path.insert(0,str(Path(__file__).parent))
import make_strips as ms
PORTRAIT=Path(__file__).resolve().parents[2]/"scripts/synthetic-game-replay/assets/mara-venn-portrait-v1.png"
def sha(b): return hashlib.sha256(b).hexdigest()
def context_mean():
    from base_frame import base
    b,_=base(); cx,cy=501,297; r=max(2,int(74*.75))
    return float(b[cy-r:cy+r,cx-r:cx+r,:3].mean())
# coefficient order: jaw, lipClose, funnel, pucker, smileL, smileR, upperRaise, lowerDepress
STATES=[('neutral-closed-transparent',[0,1,0,0,0,0,0,0],None),
        ('ah-open',[1,0,0,0,0,0,0,0],'ah'),
        ('oo-rounded',[.75,0,1,0,0,0,0,0],'oo'),
        ('ee-spread',[.575,0,0,0,1,1,0,0],'ee'),
        ('small-aperture-teeth-f-v-t-d',[.175,0,0,0,0,0,0,0],'small')]
def main():
    p=argparse.ArgumentParser(); p.add_argument('--out',type=Path,required=True)
    p.add_argument('--evidence-sha256',required=True); p.add_argument('--refine',choices=['true','false'],default='false')
    p.add_argument('--provenance',type=Path,required=True)
    a=p.parse_args()
    if a.out.exists(): sys.exit('output exists')
    ctx=context_mean()
    tex=bytearray()
    for name,coef,key in STATES:
        if key is None: tex+=np.zeros((ms.H,ms.W,4),np.uint8).tobytes()
        else:
            c,al=ms.STATES[key](); tex+=ms.premult_bgra(c,al).tobytes()
    tex=bytes(tex); tsha=sha(tex)
    prov=[sha(PORTRAIT.read_bytes()), sha(a.provenance.read_bytes())]
    binding={"schemaVersion":1,"gameProfileId":"eclipse-harbor","characterId":"mara-venn",
             "referenceProvenanceSha256":prov,"reviewStatus":"reviewed-private","reviewEvidenceSha256":a.evidence_sha256}
    seed=json.dumps(["mara-venn","eclipse-harbor",prov,tsha,a.refine]).encode()
    rev=int.from_bytes(hashlib.sha256(seed).digest()[:8],'little') or 1
    refine=a.refine=='true'
    manifest={"schemaVersion":4,"identityRevision":rev,"enrollmentBinding":binding,
      "texture":{"file":"atlas-bgra8-premultiplied.bin","sha256":tsha,"representation":"normalized-oral-strip-v1",
                 "width":ms.W,"height":ms.H,"strideBytes":ms.W*4,"stateCount":len(STATES),"stateBytes":ms.W*ms.H*4},
      "states":[{"index":i,"coefficients":c,"enrolledPose":[0,0,0],"referenceContextMean":round(ctx,4),"refineSourceEdges":refine}
                for i,(n,c,k) in enumerate(STATES)]}
    a.out.mkdir(parents=True)
    (a.out/'atlas-bgra8-premultiplied.bin').write_bytes(tex)
    mj=(json.dumps(manifest,indent=2)+"\n").encode(); (a.out/'atlas.json').write_bytes(mj)
    canon=sha(json.dumps(binding,separators=(',',':'),ensure_ascii=False).encode())
    receipt={"schema":"interactive-npcs-reviewed-mouth-atlas/v1",
      "artifact":{"atlas":{"path":"atlas.json","sizeBytes":len(mj),"sha256":sha(mj)},
                  "texture":{"path":"atlas-bgra8-premultiplied.bin","sizeBytes":len(tex),"sha256":tsha}},
      "atlasIdentity":{"schemaVersion":4,"identityRevision":str(rev),"representation":"normalized-oral-strip-v1",
                       "gameProfileId":"eclipse-harbor","characterId":"mara-venn","enrollmentBindingSha256":canon},
      "review":{"classification":"reviewed-private-character-pack","status":"accepted-for-private-review",
                "reviewEvidenceSha256":a.evidence_sha256,"naturalQualityQualified":False,"ordinaryTargetsEnabled":False,
                "qualification":("Procedurally RENDERED oral-interior strips (not observed, not photographed, no image model) for the fictional "
                  "Eclipse Harbor character Mara Venn; exterior lips always come from the current game frame. Private component-review "
                  "pack; the visual review sheet is recorded by reviewEvidenceSha256. Natural motion, live capture, "
                  "game-load behaviour and ordinary targets are unqualified.")}}
    (a.out/'reviewed-artifact-receipt.v1.json').write_text(json.dumps(receipt,indent=2)+"\n",encoding='utf-8')
    print(json.dumps({"out":str(a.out),"textureSha256":tsha,"contextMean":ctx,"identityRevision":rev,"refine":refine}))
main()
