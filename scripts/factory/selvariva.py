"""Selvariva: a fictional 6 km wooded basin and a one-hour facilitated TTX.

Uses the already transferred EUPL-1.2 terrain, vegetation, road-junction and
footprint kernels (see provenance.json). No geographical source data. Scenario
inspiration is thematic only; names, geometry and operational quantities are
new authored training assumptions, not reconstructions of the source events.
"""
from dataclasses import replace
import hashlib
import json
from pathlib import Path

import numpy as np
from . import grid, terrain, vegetation, roads
from .layout import corners, mtv
from .network import rounded, grade_roadbeds

WORLD = 6000
CELL = 20
ID = 'selvariva_basin'


def write(path, value, compact=False):
    text = json.dumps(value, ensure_ascii=False, indent=None if compact else 2,
                      separators=(',', ':') if compact else None) + '\n'
    path.write_text(text)
    return hashlib.sha256(text.encode()).hexdigest()


def network():
    specs = [
        ('fondovalle', 'Strada del Fondovalle', 'secondary', True,
         [[0,2400],[1000,2350],[1900,2150],[2800,2100],[3450,2250],[4100,2400],[4950,2500],[6000,2700]]),
        ('fonti', 'Strada delle Fonti', 'tertiary', True,
         [[1900,2150],[1600,2700],[1700,3200],[2050,3550],[1900,3800]]),
        ('belvedere', 'Strada del Belvedere', 'tertiary', True,
         [[2050,3550],[2500,3500],[2850,3300],[3300,3100],[4000,2950],[4600,2700],[4950,2500]]),
        ('uscita_sud', 'Strada dei Prati', 'tertiary', True,
         [[3450,2250],[3450,1850],[3600,1350],[3900,700],[4100,0]]),
        ('sentiero', 'Sentiero di Montebruno', 'track', False,
         [[1900,3800],[2150,4250],[2900,4550],[4000,4200],[4600,3500],[4600,2700]]),
    ]
    for i,y in enumerate([2020,2200,2400,2580]):
        specs.append((f'selva_{i}',f'Via di Selvariva {i+1}','residential',True,[[3150,y],[3450,y],[3750,y]]))
    for i,x in enumerate([3150,3450,3750]):
        specs.append((f'selva_link_{i}',f'Raccordo di Selvariva {i+1}','residential',True,[[x,2020],[x,2200],[x,2400],[x,2580]]))
    specs.extend([
        ('roveri_alta','Via delle Roveri','residential',True,[[2200,3640],[2450,3640],[2600,3600]]),
        ('roveri_bassa','Via delle Sorgenti','residential',True,[[2050,3550],[2300,3400],[2550,3350]]),
        ('roveri_link','Raccordo Roverechiare','residential',True,[[2450,3640],[2500,3500],[2550,3350]]),
    ])
    fixed=[p for *_,line in specs for p in [line[0],line[-1]]]
    result=[dict(id=rid,name=name,**{'class':cls},drivable=drive,oneway=False,
                 points=rounded(line,radius=55 if cls!='residential' else 20,fixed=fixed))
            for rid,name,cls,drive,line in specs]
    return result


def generate(template, seed=33):
    s=json.loads(json.dumps(template))
    candidate=replace(terrain.by_id('t3'), seed=seed)
    full=terrain.generate(candidate)
    eu,veg=vegetation.generate(full,seed)
    z=grid.smooth(full[50:350,50:350].copy(),2.0)
    z=150+.55*(z-z.min())
    fuel=eu[50:350,50:350].copy()
    axis=(np.arange(300)+.5)*CELL
    X,Y=np.meshgrid(axis,WORLD-axis)
    # Broad settlement terraces authored before roads/buildings, independent
    # of any fire trajectory. The peripheral slopes retain their vegetation.
    for cx,cy,radius in [(3450,2280,520),(2370,3500,290),(4900,2450,230),(1880,3800,160)]:
        weight=np.exp(-(np.hypot(X-cx,Y-cy)/radius)**6)
        level=float((z*weight).sum()/weight.sum())
        z=z*(1-.96*weight)+level*.96*weight
    net=network()
    facilities=[]
    def facility(name,kind,pos,size,people,capacity=0,visual=None):
        b=dict(id=len(facilities)+1,name=name,kind=kind,pos=pos,size=size,
               occupants=people,shelter_capacity=capacity,sector='basin')
        if visual:b['visual_kind']=visual
        facilities.append(b)
    facility('Municipio di Selvariva','town_hall',[3500,2280],[38,26],12)
    facility('Scuola di Roverechiare','school',[2400,3570],[45,30],80)
    facility('Presidio sanitario di Selvariva','hospital',[3660,2450],[48,32],28)
    facility('Polo di accoglienza dei Prati','sports_centre',[3550,1820],[65,42],0,400)
    facility('Centro di accoglienza di Pianalto','sports_centre',[4900,2400],[80,50],0,800)
    facility('Area festival delle Fonti','sports_centre',[1870,3810],[110,75],360,0,'campsite')
    facility('Rifugio della scolaresca','school',[2070,3690],[30,22],40)
    facility('Deposito delle Fornaci','industrial',[2750,3270],[65,42],16)
    facility('Cascina delle Roveri','house',[2250,3290],[27,20],8,0,'farm')
    facility('Centro operativo di Pianalto','town_hall',[4760,2520],[40,28],8)
    facility('Piazza di Roverechiare','sports_centre',[2320,3520],[54,42],0,120,'assembly')
    facility('Stazione carburanti dei Prati','fuel_station',[3720,1890],[25,22],4)
    # Facility driveways attach to an existing street vertex, with endpoints
    # outside the building footprint (never a drawn but disconnected access).
    for b in facilities:
        q=np.array(b['pos']);sx,sy=b['size']
        ends=[q+[sx/2+12,0],q+[-sx/2-12,0],q+[0,sy/2+12],q+[0,-sy/2-12]]
        vertices=np.array([p for road in net if road['drivable'] for p in road['points']])
        a,end=min(((vertices[np.argmin(np.linalg.norm(vertices-e,axis=1))],e) for e in ends),
                  key=lambda pair:np.linalg.norm(pair[0]-pair[1]))
        net.append(dict(id=f'access_{b["id"]}',name=f'Accesso {b["name"]}',**{'class':'service'},
                        drivable=True,oneway=False,points=roads.resample(np.array([a,end]),every=10).tolist()))
    adapted=[dict(r,line=r['points']) for r in net]
    for i,r in enumerate(adapted):roads.weld_crossings(r,adapted[:i])
    for r,a in zip(net,adapted):r['points']=a['line']
    grading=grade_roadbeds(z,net,max_grade=.16,cell_m=CELL)
    fixed=[corners(np.array(b['pos']),b['size'][0]+10,b['size'][1]+10,0) for b in facilities]
    road_samples=np.array([p for r in net if r['drivable'] for p in roads.resample(np.array(r['points']),every=3)])
    rng=np.random.default_rng(seed+2000)
    homes=[]
    # Independent settlements and roadside population, not copies of Valverde.
    for prefix,centre,limit,count in [('Selvariva',[3450,2300],620,105),('Roverechiare',[2370,3500],400,55)]:
        slots=[]
        for r in net:
            if not r['drivable'] or r['id'].startswith('access_'):continue
            p=roads.resample(np.array(r['points']),every=38)
            for a,b in zip(p,p[1:]):
                d=b-a;length=np.linalg.norm(d)
                if length<1:continue
                normal=np.array([-d[1],d[0]])/length
                for side in [-1,1]:
                    q=(a+b)/2+side*normal*30
                    if np.linalg.norm(q-centre)<limit:slots.append(q)
        rng.shuffle(slots)
        for q in slots:
            size=[float(rng.integers(12,19)),float(rng.integers(10,16))]
            if np.any((np.abs(road_samples[:,0]-q[0])<size[0]/2+7)&(np.abs(road_samples[:,1]-q[1])<size[1]/2+7)):continue
            box=corners(q,size[0]+10,size[1]+10,0)
            if any(mtv(box,b) is not None for b in fixed):continue
            homes.append(dict(id=len(facilities)+len(homes)+1,kind='house',pos=np.round(q,2).tolist(),size=size,
                              occupants=int(rng.integers(2,6)),shelter_capacity=0,sector='basin'))
            fixed.append(box)
            if sum(np.linalg.norm(np.array(b['pos'])-centre)<limit for b in homes)>=count:break
        # Count only the current settlement, not previous ones.
        placed=sum(np.linalg.norm(np.array(b['pos'])-centre)<limit for b in homes)
        if placed<count:raise ValueError(f'Only {placed}/{count} building sites for {prefix}')
    buildings=facilities+homes
    for b in buildings:
        x,y=b['pos'];sx,sy=b['size']
        fuel[(np.abs(X-x)<=sx/2+2)&(np.abs(Y-y)<=sy/2+2)]=0
        # Every building occupies its physical cell, even when its footprint is
        # smaller than a 20 m cell and contains no cell centre.
        fuel[min(299,int((WORLD-y)/CELL)),min(299,int(x/CELL))]=0
    # EU12 ids are retained; 0 is bare ground, 3 remains burnable grass.
    fine_nodes=np.arange(601)*10
    fine=np.stack([np.interp(fine_nodes,axis,row) for row in z])
    fine=np.stack([np.interp(fine_nodes,axis,fine[:,i]) for i in range(601)],axis=1)
    s.update(id=ID,name='Selvariva — Due Valli basin',
        description='Fully synthetic 6 × 6 km territory: wooded basin, Montebruno hill, two settlements, a festival area and three road exits. No real geography.',
        grid=dict(rows=300,cols=300,cellsize=CELL),dem=np.round(z,3).ravel().tolist(),fuel=fuel.ravel().tolist(),
        fuel_definitions=json.loads((Path(__file__).parent/'fuels_eu12.json').read_text()),
        render_terrain=dict(rows=601,cols=601,posting_m=10,world_size_m=[WORLD,WORLD],elev=np.round(fine,3).ravel().tolist()),
        roads=net,buildings=buildings,sectors=[dict(id='basin',name='Conca delle Due Valli',bounds=[0,0,WORLD,WORLD])],
        zones=dict(cols=8,rows=8,size_m=750),timeline=[],msel=[],
        start_clock_s=14*3600,turn_minutes=60,weather=dict(wind_dir_deg=315,wind_speed_kmh=32,moisture_pct=8),
        staging_base='Centro operativo di Pianalto',operational_grants=[],operations={k:k!='spotting' for k in [
            'road_routing','progressive_suppression','traffic','intensity_exposure','spotting',
            'segment_fire_closures','terrain_travel','emergency_congestion','local_autonomy']})
    s['places']=[dict(name=b['name'],pos=b['pos'],aliases=[]) for b in facilities]+[
        dict(name='Montebruno',pos=[2200,4100],aliases=['colle','bosco alto']),
        dict(name='Margine nord di Roverechiare',pos=[2340,3770],aliases=['fronte nord']),
        dict(name='Margine ovest di Roverechiare',pos=[2060,3500],aliases=['fronte ovest']),
        dict(name='Bosco delle Fonti',pos=[1700,4010],aliases=['innesco','bosco festival']),
        dict(name='Selvariva',pos=[3450,2300],aliases=['capoluogo']),
        dict(name='Roverechiare',pos=[2370,3500],aliases=['borgo']),
        dict(name='Pianalto',pos=[4900,2450],aliases=['accoglienza est'])]
    s['resource_types']=[dict(id=rid,name=name,description=desc,speed_mps=speed) for rid,name,desc,speed in [
        ('ground_crews','Forest crews','Crews for fire lines, with travel and progressive work.',1.4),
        ('air_tankers','Air tankers','Point drops, with travel and return for refilling.',55),
        ('engines','Engines','Road missions with limited water and refilling at the base.',10),
        ('buses','Buses','50 seats; needed for schools and the health post. The model does not reuse buses for shuttles.',10),
        ('police_patrols','Patrols','Closures to civilian traffic. They do not repair roads.',12),
        ('drones','Monitoring drones','Keep the map of up to 4 zones updated every 5 minutes while on station (40 minutes), then return and recharge for 15.',20)]]
    for r in s['resource_types']:
        if r['id']=='drones':r['drone']=dict(max_zones=4,endurance_s=2400,turnaround_s=900,update_interval_s=300)
    s['roles']=[dict(id=rid,name=name,level=level,language='en',description=desc,allowed_actions=actions,resources=res) for rid,name,level,desc,actions,res in [
        ('mayor','Municipal coordination','local','Warnings, evacuation and reception in the two settlements. Agrees priorities and routes with the fire director.',
         ['IssueWarning','EvacuateZone','EvacuateBuilding','OpenShelter','CloseRoad','ReopenRoad'],{'buses':4,'police_patrols':3}),
        ('fire_commander','Fire command','regional','Choose where to commit crews, engines, aircraft and the monitoring drone; transfers and safety limit the missions.',
         ['CutFireLine','WaterDrop','DeployEngine','MonitorArea','Reconnoitre'],{'ground_crews':4,'air_tankers':1,'engines':2,'drones':1}),
        ('regional_cp','Coordination room','national','Gather the situation picture, request reinforcements and coordinate public communication and external assistance.',
         ['RequestReinforcement','IssueWarning','OpenShelter'],{}),
        ('volunteer_coordinator','Assistance coordination','local','Organises assistance to families, psychological support and needs assessment through narrative initiatives submitted to EXCON. No medical or search mission is simulated automatically.',[],{})]]
    s['excon_roles']=['Reporting centre','Due Valli weather service','Field observers','Festival management','Assistance network','Corriere della Conca newsroom']
    s['reinforcements']=[dict(resource=r,max_qty=q,delay_min=t,source=src,assigned_to=role) for r,q,t,src,role in [
        ('buses',2,35,'Due Valli transport consortium','mayor'),('ground_crews',2,45,'Roccaluce fire station','fire_commander'),
        ('engines',1,50,'Roccaluce fire station','fire_commander'),('air_tankers',1,75,'Altavela air base','fire_commander')]]
    # Display texts are English; Italian readers get these (English source -> Italian).
    s['translations']={'it':{
        "Corriere della Conca newsroom": "Redazione del Corriere della Conca",
        "Selvariva — Due Valli basin": "Selvariva — conca delle Due Valli",
        "Fully synthetic 6 × 6 km territory: wooded basin, Montebruno hill, two settlements, a festival area and three road exits. No real geography.": "Territorio interamente sintetico di 6 × 6 km: conca boscata, colle di Montebruno, due abitati, area festival e tre uscite stradali. Nessuna geografia reale.",
        "Forest crews": "Squadre boschive",
        "Crews for fire lines, with travel and progressive work.": "Squadre per linee tagliafuoco, con viaggio e lavoro progressivo.",
        "Air tankers": "Mezzi aerei",
        "Point drops, with travel and return for refilling.": "Lanci puntuali con viaggio e rientro per rifornimento.",
        "Engines": "Autobotti",
        "Road missions with limited water and refilling at the base.": "Missioni stradali con acqua limitata e rifornimento alla base.",
        "Buses": "Autobus",
        "50 seats; needed for schools and the health post. The model does not reuse buses for shuttles.": "50 posti; necessari per scuole e presidio sanitario. Il modello non riusa gli autobus con navette.",
        "Patrols": "Pattuglie",
        "Closures to civilian traffic. They do not repair roads.": "Chiusure al traffico civile. Non riparano le strade.",
        "Monitoring drones": "Droni di monitoraggio",
        "Keep the map of up to 4 zones updated every 5 minutes while on station (40 minutes), then return and recharge for 15.": "Mantengono aggiornata la mappa di al massimo 4 zone ogni 5 minuti mentre sono in posizione (40 minuti), poi rientrano e si ricaricano per 15.",
        "Municipal coordination": "Coordinamento comunale",
        "Warnings, evacuation and reception in the two settlements. Agrees priorities and routes with the fire director.": "Avvisi, evacuazione e accoglienza nei due abitati. Concorda priorità e percorsi con il direttore antincendio.",
        "Fire command": "Direzione antincendio",
        "Choose where to commit crews, engines, aircraft and the monitoring drone; transfers and safety limit the missions.": "Scegli dove impegnare squadre, autobotti, mezzo aereo e drone di monitoraggio; i trasferimenti e la sicurezza limitano le missioni.",
        "Coordination room": "Sala di coordinamento",
        "Gather the situation picture, request reinforcements and coordinate public communication and external assistance.": "Raccogli il quadro di situazione, richiedi rinforzi e coordina comunicazione pubblica e assistenza esterna.",
        "Assistance coordination": "Coordinamento assistenza",
        "Organises assistance to families, psychological support and needs assessment through narrative initiatives submitted to EXCON. No medical or search mission is simulated automatically.": "Organizza assistenza alle famiglie, supporto psicologico e raccolta dei bisogni con iniziative narrative sottoposte a EXCON. Nessuna missione sanitaria o ricerca è simulata automaticamente.",
        "Due Valli transport consortium": "Consorzio trasporti delle Due Valli",
        "Roccaluce fire station": "Distaccamento di Roccaluce",
        "Altavela air base": "Centro aereo di Altavela",
        "Reporting centre": "Centrale segnalazioni",
        "Due Valli weather service": "Servizio meteo delle Due Valli",
        "Field observers": "Osservatori sul territorio",
        "Festival management": "Gestione festival",
        "Assistance network": "Rete di assistenza"
}}
    def elevation(p):
        c,r=np.clip([p[0]/CELL-.5,(WORLD-p[1])/CELL-.5],0,299);i,j=int(r),int(c);v,u=r-i,c-j
        return ((1-v)*((1-u)*z[i,j]+u*z[i,min(j+1,299)])+v*((1-u)*z[min(i+1,299),j]+u*z[min(i+1,299),min(j+1,299)]))
    grades=[abs(elevation(a)-elevation(b))/np.linalg.norm(np.array(a)-b)
            for r in net if r['drivable'] for a,b in zip(r['points'],r['points'][1:]) if np.linalg.norm(np.array(a)-b)>1]
    if max(grades)>.19:raise ValueError(f'Road grade exceeds 19%: {max(grades)}')
    return s,dict(recipe='selvariva-basin-v1',seed=seed,candidate=vars(candidate),vegetation=veg,grading=grading,
        physical_cell_m=CELL,visual_posting_m=10,world_size_m=[WORLD,WORLD],buildings=len(buildings),roads=len(net),
        occupants=sum(b['occupants'] for b in buildings),elevation_range_m=[float(z.min()),float(z.max())],
        max_drivable_grade=max(grades),provenance='Existing TTX Factory kernels, adapted from propagator_abm under EUPL-1.2; all geography and demographic counts synthetic.')


def landscape(args):
    template=json.loads(args.template.read_text())
    if args.output.resolve()==args.template.resolve():raise ValueError('Output must differ from template')
    s,manifest=generate(template,args.seed)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    manifest['scenario_sha256']=write(args.output,s,compact=True)
    manifest['template_sha256']=hashlib.sha256(args.template.read_bytes()).hexdigest()
    # The exercise pack is authored by hand; the manifest only records its hash.
    pack=args.output.parent/'selvariva_crisi_integrata.pack.json'
    if pack.exists():manifest['pack_sha256']=hashlib.sha256(pack.read_bytes()).hexdigest()
    write(args.output.with_suffix('.manifest.json'),manifest)
    print(json.dumps(manifest,indent=2,ensure_ascii=False))
