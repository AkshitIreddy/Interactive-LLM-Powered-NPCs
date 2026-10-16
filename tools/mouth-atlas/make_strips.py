"""Procedural oral-interior strips (128x64) for Mara Venn. Rendered, not observed."""
import cv2, numpy as np
W,H=128,64
U=(np.arange(W)+.5)/W
V=(np.arange(H)+.5)/H
UU,VV=np.meshgrid(U,V)
def ss(x,a,b):
    t=np.clip((x-a)/(b-a),0,1); return t*t*(3-2*t)
def rgb(r,g,b): return np.array([b,g,r],np.float32)   # BGR
def blur(a,s): return cv2.GaussianBlur(a,(0,0),s)
CAV_EDGE=rgb(78,34,32)      # warm deep brown-red near the lips
CAV_DEEP=rgb(26,9,10)       # darker towards the centre
TEETH=rgb(138,119,98)      # desaturated warm ivory, ~48% luminance (ceiling 55-65%)
TONGUE=rgb(104,52,54)       # muted dark rose
LIPIN=rgb(104,58,56)        # inner lower-lip tone
def cavity(depth_u=1.0):
    # darkest in the middle of the strip, warm near top/bottom/corners
    dc=np.sqrt(((UU-.5)/.5)**2*.55+((VV-.5)/.5)**2*.9)
    t=ss(1-dc,0.0,0.75)[...,None]
    return CAV_EDGE*(1-t)+CAV_DEEP*t
def teeth_band(top,bottom,curve=.55,shade_top=.55):
    """Soft tooth band between rows top..bottom (fractions). Incisal edge curves
    upward towards the corners; darker at corners and just under the upper lip."""
    side=np.abs(2*UU-1)
    edge_bottom=bottom-curve*(bottom-top)*ss(side,.25,1.0)**1.6
    a=ss(VV,top-.02,top+.05)*(1-ss(VV,edge_bottom-.05,edge_bottom+.05))
    a*=1-ss(side,.80,1.0)                      # fades into the buccal corridor
    lum=(1-.50*ss(side,.15,.95))               # shaded at the corners
    lum*=(shade_top+(1-shade_top)*ss(VV,top,top+(bottom-top)*.55))  # shade under upper lip
    lum*=(1-.18*ss(VV,top+(bottom-top)*.6,bottom))                 # soft shade toward gum edge
    return blur(a,1.2), lum[...,None]
def finish(col,alpha):
    alpha=np.clip(alpha,0,1)
    # feather the outermost ring so the current-frame lips blend
    alpha=alpha*ss(UU,0.0,.05)*(1-ss(UU,.95,1.0))*ss(VV,0.0,.06)*(1-ss(VV,.94,1.0))
    col=np.clip(col,0,255)
    return col.astype(np.float32),alpha.astype(np.float32)
def state_ah():
    col=cavity()
    a,l=teeth_band(.0,.34,shade_top=.42)
    col=col*(1-a[...,None])+TEETH*l*a[...,None]
    # tongue: muted rose ellipse low in the cavity, soft edges
    d=np.sqrt(((UU-.5)/.30)**2+((VV-.80)/.24)**2)
    t=(1-ss(d,.55,1.0))
    groove=1-.18*np.exp(-((UU-.5)/.015)**2)
    tong=TONGUE*((.75+.25*(1-VV))*groove)[...,None]
    tongue_a=(t*.85)[...,None]
    col=col*(1-tongue_a)+tong*tongue_a
    # faint lower-teeth hint at the very bottom corners of the cavity
    alpha=ss(1-np.abs(2*UU-1),.03,.22)
    return finish(col,alpha)
EE_TEETH=rgb(122,112,100)   # desaturated warm grey-ivory, ~44% luminance peak
def state_ee():
    col=cavity()
    side=np.abs(2*UU-1)
    # half-height upper-teeth band at the top; dark cavity shows below it
    bottom=.44-.14*ss(side,.3,1.0)
    a=ss(VV,-.02,.06)*(1-ss(VV,bottom-.07,bottom+.07))*(1-ss(side,.68,.93))
    a=blur(a,1.1)
    lum=(1-.62*ss(side,.10,.90))                       # darker at both corners
    lum=lum*(.50+.50*ss(VV,0.0,.14))*(1-.55*ss(VV,.12,bottom+.04))   # shade under lip and toward the inner edge
    col=col*(1-a[...,None])+EE_TEETH*lum[...,None]*a[...,None]
    alpha=ss(1-side,.02,.18)
    return finish(col,alpha)
def state_oo():
    col=cavity()
    # round opening: the visible cavity is an ellipse, alpha falls off to the lips
    d=np.sqrt(((UU-.5)/.5)**2+((VV-.5)/.5)**2)
    alpha=1-ss(d,.78,1.0)
    return finish(col,alpha)
def state_small():   # f/v and other small-aperture consonants
    col=np.broadcast_to(LIPIN,(H,W,3)).copy()
    side=np.abs(2*UU-1)
    a,l=teeth_band(.0,.72,curve=.30,shade_top=.62)
    # teeth rest on the lower lip: bottom edge is the lip, so keep the band solid down to it
    col=col*(1-a[...,None]*.80)+TEETH*(l*.72)*a[...,None]*.80
    # thin dark line where the lip meets the teeth
    line=np.exp(-((VV-.74)/.05)**2)*(1-ss(side,.8,1.0))
    col=col*(1-.35*line[...,None])
    alpha=ss(1-side,.03,.2)
    return finish(col,alpha)
STATES={'ah':state_ah,'oo':state_oo,'ee':state_ee,'small':state_small}
def premult_bgra(col,alpha):
    a8=np.rint(alpha*255).astype(np.uint8)
    pm=np.rint(col*(a8[...,None]/255.0)).astype(np.uint8)
    return np.dstack([pm,a8])
if __name__=='__main__':
    sheet=[]
    chk=((np.indices((H,W)).sum(0)//8)%2*30+40).astype(np.float32)
    for k,f in STATES.items():
        c,a=f(); vis=c*a[...,None]+chk[...,None]*(1-a[...,None])
        vis=cv2.resize(vis.astype(np.uint8),(W*4,H*4),interpolation=cv2.INTER_NEAREST)
        cv2.putText(vis,k,(6,18),cv2.FONT_HERSHEY_SIMPLEX,.6,(255,255,255),1); sheet.append(vis)
    cv2.imwrite('strips_preview.png',np.vstack(sheet))
