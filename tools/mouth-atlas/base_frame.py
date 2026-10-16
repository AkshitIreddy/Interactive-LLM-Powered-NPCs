"""Mara portrait rendered at the practice game's 960x600 client size."""
import cv2, numpy as np, os
from pathlib import Path
SRC=str(Path(__file__).resolve().parents[2]/"scripts/synthetic-game-replay/assets/mara-venn-portrait-v1.png")
def base(w=960,h=600):
    im=cv2.imread(SRC)
    H,W=im.shape[:2]
    s=max(w/W,h/H); sw,sh=w/s,h/s
    x0,y0=(W-sw)/2,(H-sh)/2
    M=np.float32([[s,0,-x0*s],[0,s,-y0*s]])
    return cv2.warpAffine(im,M,(w,h),flags=cv2.INTER_AREA if s<1 else cv2.INTER_CUBIC), (s,x0,y0)
def write_ppm(path,img):
    h,w=img.shape[:2]
    with open(path,'wb') as f:
        f.write(b'P6\n%d %d\n255\n'%(w,h)); f.write(img[:,:,::-1].tobytes())
if __name__=='__main__':
    b,t=base(); print(t)
    cv2.imwrite('base960.png',b)
    os.makedirs('lm_in',exist_ok=True)
    for i in range(1,13): write_ppm(f'lm_in/frame-{i:05d}.ppm',b)
