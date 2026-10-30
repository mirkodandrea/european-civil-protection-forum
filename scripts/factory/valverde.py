"""Reproducible TTX landscape recipe using the transferred Factory kernels.

Keeps the exercise's named facilities, roles and MSEL. The historical scenario
is a template, never an output. Physical DEM and fuels are regenerated together;
the 5 m visual surface only interpolates the physical 20 m DEM.
"""
from dataclasses import replace
import hashlib
import json

import numpy as np

from . import grid, terrain, vegetation, roads, districts
from .network import generate as road_network, grade_roadbeds
from .layout import corners, mtv


def generate(template, seed=11):
    scn = json.loads(json.dumps(template))
    candidate = replace(terrain.by_id('t1'), seed=seed)
    full = terrain.generate(candidate)
    eu, veg = vegetation.generate(full, seed)
    # Central 4 km, preserving the TTX coordinate frame and named places.
    z = full[100:300, 100:300].copy()
    z = grid.smooth(z, 3.2)
    z = 70 + .72 * (z - z.min())
    X,Y = grid.xy()
    X,Y = X[100:300,100:300]-2000,Y[100:300,100:300]-2000
    # Authored settlement terraces make real streets possible without tiny
    # raster switchbacks through the existing named facilities.
    for cx,cy,radius in [(1600,1300,560),(2800,2250,260)]:
        w=np.exp(-(np.hypot(X-cx,Y-cy)/radius)**6)
        level=float((z*w).sum()/w.sum())
        z=z*(1-.98*w)+level*.98*w
    # Explicit translation into TTX's calibrated legacy fuel table.
    mapping = np.array([3, 4, 6, 4, 7, 1, 1, 2, 2, 2, 5, 5, 5])
    fuel = mapping[eu[100:300, 100:300]]
    n, cell = z.shape[0], 20.0

    def elevation(p):
        # Same cell-centred bilinear convention used by the terrain surface.
        c, r = np.clip(p[0]/cell-.5, 0, n-1), np.clip((4000-p[1])/cell-.5, 0, n-1)
        i,j = int(r),int(c)
        v,w = r-i,c-j
        return ((1-v)*((1-w)*z[i,j]+w*z[i,min(j+1,n-1)]) +
                v*((1-w)*z[min(i+1,n-1),j]+w*z[min(i+1,n-1),min(j+1,n-1)]))

    network = road_network()

    def road(rid,name,stops,cls='service',drivable=True):
        points=roads.resample(np.array(stops,dtype=float),every=8).tolist()
        network.append(dict(id=rid,name=name,points=points,**{'class':cls},drivable=drivable,oneway=False))

    # Dedicated accesses terminate outside the actual facility footprint.
    for b in scn['buildings'][:6]:
        p = b['pos']; sx,sy=b['size']
        ends=[[p[0],p[1]-sy/2-12],[p[0],p[1]+sy/2+12],
              [p[0]-sx/2-12,p[1]],[p[0]+sx/2+12,p[1]]]
        vertices = [q for r in network if r['drivable'] for q in r['points']]
        choices = [(min(vertices,key=lambda q: np.linalg.norm(np.array(q)-end)),end) for end in ends]
        choices.sort(key=lambda pair: np.linalg.norm(np.array(pair[0])-pair[1]))
        for start,end in choices:
            try:
                road(f'access_{b["id"]}',f'Access to {b["name"]}',[start,end])
                break
            except (ValueError, RuntimeError):
                continue
        else:
            raise ValueError(f'No feasible facility access: {b["name"]}')
    adapted = [dict(r,line=r['points']) for r in network]
    for i,r in enumerate(adapted):
        roads.weld_crossings(r,adapted[:i])
    for r,a in zip(network,adapted):
        r['points'] = a['line']
    scn['roads'] = network
    grading=grade_roadbeds(z,network)

    # Candidate homes alongside real roads. Actual axis-aligned TTX footprints
    # are checked with the transferred separating-axis geometry.
    fixed = [corners(np.array(b['pos']),*b['size'],0) for b in scn['buildings'][:6]]
    segments = [(np.array(a),np.array(b)) for r in network for a,b in zip(r['points'],r['points'][1:])]
    road_samples = np.concatenate([a+np.linspace(0,1,max(2,int(np.linalg.norm(b-a)/2)+1))[:,None]*(b-a)
                                   for a,b in segments])
    def clear_street(p, size):
        return not np.any((np.abs(road_samples[:,0]-p[0])<size[0]/2+8)&
                          (np.abs(road_samples[:,1]-p[1])<size[1]/2+8))
    slots=[]
    for r in network:
        if not r['drivable'] or r['id'].startswith('access_'): continue
        p=roads.resample(np.array(r['points']),every=42)
        for a,b in zip(p,p[1:]):
            d=b-a; length=np.linalg.norm(d)
            if length<1: continue
            normal=np.array([-d[1],d[0]])/length
            for sign in [-1,1]:
                q=(a+b)/2+sign*normal*28
                if np.all(q>80) and np.all(q<3920) and clear_street(q,[14,12]):
                    slots.append(q)
    for b in scn['buildings'][6:]:
        target=np.array(b['pos'])
        order=sorted(range(len(slots)),key=lambda i:np.linalg.norm(slots[i]-target))
        for i in order:
            q=slots[i]; box=corners(q,b['size'][0]+8,b['size'][1]+8,0)
            if all(mtv(box,f) is None for f in fixed):
                b['pos']=[round(float(v),2) for v in q]
                fixed.append(box); slots.pop(i); break
        else:
            raise ValueError('Insufficient non-overlapping roadside building sites')
    for b in scn['buildings']:
        x,y=b['pos']; sx,sy=b['size']
        # Clear the entire footprint, not a single point under a large building.
        c0,c1=np.clip([int((x-sx/2)/cell),int((x+sx/2)/cell)+1],0,n)
        r0,r1=np.clip([int((4000-y-sy/2)/cell),int((4000-y+sy/2)/cell)+1],0,n)
        fuel[r0:r1,c0:c1]=3
        b['sector']=next(s['id'] for s in scn['sectors'] if s['bounds'][0]<=x<=s['bounds'][2] and s['bounds'][1]<=y<=s['bounds'][3])
    # At model resolution, no invented high-frequency physical barriers.
    source=np.arange(n)*cell+cell/2
    nodes=np.arange(801)*5
    fine=np.stack([np.interp(nodes,source,row) for row in z])
    fine=np.stack([np.interp(nodes,source,fine[:,i]) for i in range(len(nodes))],axis=1)
    scn.update(id='valverde_factory',name='Valverde — valle dei Castagni (synthetic)',
               description='Synthetic Factory landscape: asymmetric wooded valley, tributary ravines, ridge, agricultural terraces and connected roadside settlements. TTX exercise templates retained; fire timing requires facilitation review.',
               dem=z.ravel().tolist(),fuel=fuel.ravel().tolist(),
               render_terrain=dict(rows=801,cols=801,posting_m=5,world_size_m=[4000,4000],elev=fine.ravel().tolist()))
    # Same incident location and weather; do not force an impact by editing fuel.
    ign=next(a['command']['at'] for i in scn['msel'] for a in i.get('actions',[]) if a['command']['type']=='Ignite'); r=int((4000-ign['y'])/20); c=int(ign['x']/20)
    if fuel[r,c]==3: raise ValueError('Opening ignition is on non-burnable ground')
    grades=[abs(elevation(a)-elevation(b))/np.linalg.norm(np.array(a)-b)
            for r in network if r['drivable'] for a,b in zip(r['points'],r['points'][1:]) if np.linalg.norm(np.array(a)-b)>1]
    if max(grades) > .18:
        worst=max((abs(elevation(a)-elevation(b))/np.linalg.norm(np.array(a)-b),r['id'],a,b) for r in network if r['drivable'] for a,b in zip(r['points'],r['points'][1:]) if np.linalg.norm(np.array(a)-b)>1)
        raise ValueError(f'Road exceeds 18% grade: {worst}')
    manifest=dict(recipe='valverde-v2-authored-network',grading=grading,seed=seed,candidate=vars(candidate),vegetation=veg,
                  fuel_translation=mapping.tolist(),physical_cell_m=20,visual_posting_m=5,
                  buildings=len(scn['buildings']),roads=len(network),occupants=sum(b['occupants'] for b in scn['buildings']),
                  elevation_range_m=[float(z.min()),float(z.max())],max_drivable_grade=max(grades))
    districts.valverde(scn)
    return scn,manifest


def landscape(args):
    if args.output.resolve() == args.template.resolve():
        raise ValueError('Output must differ from the historical template')
    template=json.loads(args.template.read_text())
    if template['grid'] != dict(rows=200,cols=200,cellsize=20):
        raise ValueError('The Valverde recipe requires the 4 km, 20 m TTX template')
    scn,manifest=generate(template,args.seed)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    text=json.dumps(scn,ensure_ascii=False,separators=(',',':'))+'\n'
    args.output.write_text(text)
    manifest['scenario_sha256']=hashlib.sha256(text.encode()).hexdigest()
    manifest['template_sha256']=hashlib.sha256(args.template.read_bytes()).hexdigest()
    args.output.with_suffix('.manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps(manifest,ensure_ascii=False,indent=2))
