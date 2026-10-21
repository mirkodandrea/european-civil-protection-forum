"""Authored Valverde road hierarchy; terrain search is a planning tool only.

A shared junction is intentional. Residential streets serve a settlement;
regional alternatives connect different entrances, without converging fans.
"""
import numpy as np
from . import roads


def rounded(points, radius=65, fixed=()):
    p=np.array(points,dtype=float)
    locked={tuple(v) for v in fixed}
    out=[p[0]]
    for a,b,c in zip(p,p[1:],p[2:]):
        u,v=b-a,c-b
        lu,lv=np.linalg.norm(u),np.linalg.norm(v)
        if lu<.01 or lv<.01: continue
        u,v=u/lu,v/lv
        angle=np.arccos(np.clip(u@v,-1,1))
        if tuple(b) in locked or angle<.02:
            out.append(b); continue
        cut=min(radius*np.tan(angle/2),.35*lu,.35*lv)
        q,r=b-cut*u,b+cut*v
        out.append(q)
        for t in np.linspace(0,1,max(6,int(cut/3)))[1:]:
            out.append((1-t)**2*q+2*t*(1-t)*b+t*t*r)
    out.append(p[-1])
    result=[out[0].tolist()]
    for a,b in zip(out,out[1:]):
        steps=max(1,int(np.ceil(np.linalg.norm(b-a)/8)))
        result.extend((a+(b-a)*t).tolist() for t in np.linspace(0,1,steps+1)[1:])
    return result


def generate():
    specs=[
        ('sp1','SP1 Main Road','secondary',True,[[0,1300],[600,1280],[1200,1300],[1600,1300],[2000,1350],[2600,1420],[3000,1440],[3500,1420],[4000,1500]]),
        ('north_road','Northern Road','tertiary',True,[[2000,1350],[2200,1600],[2350,1900],[2400,2040],[2580,2160],[2800,2150],[2920,2300],[3050,2460],[3160,2650],[3100,2840],[2870,3010],[2900,3210],[3300,3400]]),
        ('south_road','Southern Road','tertiary',True,[[1670,1000],[1550,750],[1500,400],[1400,0]]),
        ('valley_loop','Strada dei Castagni','tertiary',True,[[1200,1300],[1000,1600],[1000,1820],[1300,1950],[1700,1980],[2150,2000],[2400,2040]]),
        ('pineta_bypass','Pineta Eastern Road','tertiary',True,[[3000,1440],[3200,1650],[3200,1950],[3050,2120],[2800,2150]]),
        ('ridge_track','Ridge Track','track',False,[[2000,3300],[2400,3400],[2870,3010],[3300,3400],[4000,3250]]),
    ]
    # Separate local streets, with real intersections rather than four loops
    # all pulled toward the same pair of remote endpoints.
    for i,y in enumerate([1000,1200,1500,1620]):
        specs.append((f'town_row_{i}',f'Valverde street {i+1}','residential',True,[[1250,y],[1670,y],[1950,y]]))
    for i,x in enumerate([1250,1670,1950]):
        specs.append((f'town_link_{i}',f'Valverde local link {i+1}','residential',True,[[x,1000],[x,1200],[x,1500],[x,1620]]))
    specs += [
        ('pineta_lane','Via della Pineta','residential',True,[[2580,2300],[2800,2300]]),
        ('pineta_west','Pineta western local link','residential',True,[[2580,2160],[2580,2300]]),
        ('pineta_east','Pineta eastern local link','residential',True,[[2800,2150],[2800,2300]]),
    ]
    # Mandatory trunk junctions stay in the physical geometry exactly.
    fixed=[points[0] for *_,points in specs]+[points[-1] for *_,points in specs]
    result=[]
    for rid,name,cls,drivable,points in specs:
        result.append(dict(id=rid,name=name,**{'class':cls},drivable=drivable,oneway=False,
                           points=rounded(points,radius=65 if cls!='residential' else 22,fixed=fixed)))
    # Add endpoints explicitly even when uniform sampling stepped over them.
    # The crossing welder later joins all interior crossings too.
    for r in result:
        for point in fixed:
            if point==r['points'][0] or point==r['points'][-1]: continue
            for i,(a,b) in enumerate(zip(r['points'],r['points'][1:])):
                a,b,q=np.array(a),np.array(b),np.array(point)
                d=b-a;t=(q-a)@d/max(d@d,.001)
                if 0<t<1 and np.linalg.norm(a+t*d-q)<.01:
                    r['points'].insert(i+1,list(point));break
    return result


def grade_roadbeds(dem, network, max_grade=.15):
    """Cut/fill the synthetic 20 m terrain to the authored road network.

    Project bilinear road-height differences onto grade constraints. Shared
    junctions read the same DEM; this is generation, never renderer physics.
    """
    original=dem.copy()
    z=dem.ravel()
    n=dem.shape[0]
    def weights(p):
        c,r=np.clip([p[0]/20-.5,(4000-p[1])/20-.5],0,n-1)
        i,j=int(r),int(c);u,v=c-j,r-i
        result={}
        for ri,cj,w in [(i,j,(1-u)*(1-v)),(i,min(j+1,n-1),u*(1-v)),
                        (min(i+1,n-1),j,(1-u)*v),(min(i+1,n-1),min(j+1,n-1),u*v)]:
            result[ri*n+cj]=result.get(ri*n+cj,0)+w
        return result
    constraints=[]
    for road in network:
        if not road['drivable']: continue
        for a,b in zip(road['points'],road['points'][1:]):
            a,b=np.array(a),np.array(b)
            length=np.linalg.norm(b-a)
            if length<1:continue
            samples=a+np.linspace(0,1,max(2,int(np.ceil(length/5))+1))[:,None]*(b-a)
            for a,b in zip(samples,samples[1:]):
                w=weights(b)
                for i,v in weights(a).items():w[i]=w.get(i,0)-v
                if sum(v*v for v in w.values())>1e-12:
                    constraints.append((w,max_grade*np.linalg.norm(b-a)))
    # ADMM projection onto all road-grade constraints together. A sparse
    # matrix-free CG solve distributes cut/fill coherently along connected
    # roads instead of leaving the slow checkerboard of per-edge relaxation.
    active=list(range(z.size))
    lookup={i:i for i in active}
    ids=np.zeros((len(constraints),8),dtype=int)
    coef=np.zeros_like(ids,dtype=float)
    bound=np.array([b for _,b in constraints])
    for row,(w,_) in enumerate(constraints):
        for k,(i,v) in enumerate(w.items()):ids[row,k],coef[row,k]=lookup[i],v
    def A(x):return (x[ids]*coef).sum(axis=1)
    def AT(y):return np.bincount(ids.ravel(),weights=(y[:,None]*coef).ravel(),minlength=len(active))
    base=z.copy();x=base.copy();u=np.zeros(len(bound));d=np.clip(A(x),-bound,bound)
    rho=2000.; shoulder=40.
    def laplace(v):
        v=v.reshape(dem.shape);out=np.zeros_like(v)
        dx=v[:,1:]-v[:,:-1];dy=v[1:]-v[:-1]
        out[:,1:]+=dx;out[:,:-1]-=dx;out[1:]+=dy;out[:-1]-=dy
        return out.ravel()
    def matrix(v):return v+shoulder*laplace(v)+rho*AT(A(v))
    degree=np.full(dem.shape,4.);degree[[0,-1],:]-=1;degree[:,[0,-1]]-=1
    diagonal=1+shoulder*degree.ravel()+rho*np.bincount(ids.ravel(),weights=(coef*coef).ravel(),minlength=len(active))
    prior=base+shoulder*laplace(base)
    residual=1.
    for iteration in range(240):
        rhs=prior+rho*AT(d-u)
        r=rhs-matrix(x);q=r/diagonal;p=q.copy();rq=r@q
        for _ in range(100):
            if rq<1e-12:break
            ap=matrix(p);alpha=rq/max(p@ap,1e-20)
            x+=alpha*p;r-=alpha*ap;q=r/diagonal
            next_rq=r@q
            p=q+(next_rq/rq)*p;rq=next_rq
        ax=A(x);d=np.clip(ax+u,-bound,bound);u+=ax-d
        residual=float(np.max(np.abs(ax-d)))
        if residual<.003:break
    z[active]=x
    change=dem-original
    return dict(max_cut_fill_m=float(np.abs(change).max()),rms_cut_fill_m=float(np.sqrt((change**2).mean())),
                grade_iterations=iteration+1,grade_residual_m=residual)
