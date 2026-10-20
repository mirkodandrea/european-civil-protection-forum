// EXCON review workflow. Compute never publishes; recipients and a review source
// are explicitly selected for each frozen-generation draft.
import {api, esc, MapView} from './common.js?v=role-access-2';
export function forecastPanel(host, stat, getState) {
  host.innerHTML=`<h2>Forecast review</h2>
    <p class="small">Separate projection from the frozen current state. Current interventions persist; future team work is excluded. Pause the exercise clock before computing. Review the assumptions and outlines before publishing to selected roles.</p>
    <label>Weather groups <input data-members type="number" min="1" max="16" value="4"></label>
    <label>Horizons (minutes, increasing) <input data-horizons value="60,180"></label>
    <details><summary>Weather bulletin and perturbations</summary>
      <label>Bulletin JSON (relative seconds and weather) <textarea data-bulletin>[]</textarea></label>
      <p class="small">Example: [{"after_s":3600,"weather":{"wind_dir_deg":45,"wind_speed_kmh":35,"moisture_pct":8}}]. Empty keeps the current weather.</p>
      <label>Direction spread (degrees) <input data-direction type="number" min="0" max="90" value="10"></label>
      <label>Speed spread (fraction) <input data-speed type="number" min="0" max="0.9" step="0.05" value="0.1"></label>
    </details>
    <button data-compute>Compute private draft</button><p data-status class="small" role="status" aria-live="polite"></p>
    <div data-review hidden><p data-assumptions class="small"></p><div data-summary class="small"></div>
      <div class="mapbox"><canvas data-preview aria-label="Private forecast preview"></canvas></div>
      <p class="small">Dashed purple outlines: ensemble probability ≥50%, labelled by future horizon. The fire raster underneath is the current situation.</p>
      <fieldset><legend>Publish to selected participants</legend>${stat.roles.map(r=>`<label><input type="checkbox" data-role value="${esc(r.id)}"> ${esc(r.name)}</label>`).join('')}</fieldset>
      <label>Reviewed information source / rationale <input data-source maxlength="2000" placeholder="Bulletin reference and review rationale"></label>
      <label><input data-reviewed type="checkbox"> I reviewed this draft and its assumptions.</label>
      <button data-publish disabled>Publish reviewed forecast</button>
    </div>`;
  const q=selector=>host.querySelector(selector);
  let draft=null, preview=null, busy=false;
  const status=(text,error=false)=>{q('[data-status]').textContent=text;q('[data-status]').classList.toggle('error',error);};
  const allowPublish=()=>{q('[data-publish]').disabled=busy || !draft || !q('[data-reviewed]').checked || !q('[data-source]').value.trim() || !host.querySelector('[data-role]:checked');};
  host.addEventListener('input',allowPublish);
  q('[data-compute]').onclick=async()=>{
    busy=true;draft=null;q('[data-review]').hidden=true;allowPublish();q('[data-compute]').disabled=true;status('Computing a private draft…');
    try {
      if (!getState()?.world) throw Error('Current situation unavailable. Wait for the exercise connection.');
      const horizons=q('[data-horizons]').value.split(',').map(s=>Number(s.trim())*60);
      if(horizons.some(t=>!Number.isInteger(t)||t<1)||!horizons.length) throw Error('Enter positive horizon minutes separated by commas.');
      draft=await api('/api/master/forecast',{seed:1,members:Number(q('[data-members]').value),horizons_s:horizons,
        direction_spread_deg:Number(q('[data-direction]').value),speed_spread_fraction:Number(q('[data-speed]').value),bulletin:JSON.parse(q('[data-bulletin]').value)});
      const forecast=draft.forecast;
      q('[data-assumptions]').textContent=`Issued at t=${forecast.issued_s}s. ${forecast.assumptions}`;
      const area=stat.grid.cellsize**2/10000;
      q('[data-summary]').innerHTML=forecast.horizons.map(h=>`<p>+${h.after_s/60} min · ≥50% reached area ${(h.probability.filter(p=>p>=0.5).length*area).toFixed(1)} ha · any run ${(h.probability.filter(p=>p>0).length*area).toFixed(1)} ha</p>`).join('');
      q('[data-reviewed]').checked=false;q('[data-review]').hidden=false;
      preview ||= new MapView(q('[data-preview]'),stat,{maxHeight:420});
      preview.setView({...getState().world,forecast_products:[forecast]});preview.resize();
      status('Private draft ready. Review before publishing.');
    } catch(e) {draft=null;status(e.message,true);}
    finally {busy=false;q('[data-compute]').disabled=false;allowPublish();}
  };
  q('[data-publish]').onclick=async()=>{
    if(!draft)return;
    busy=true;allowPublish();
    try {
      await api('/api/master/forecast/publish',{id:draft.id,generation:draft.generation,
        roles:[...host.querySelectorAll('[data-role]:checked')].map(i=>i.value),source:q('[data-source]').value.trim()});
      draft=null;q('[data-reviewed]').checked=false;status('Reviewed forecast published to the selected roles.');
    }catch(e){draft=null;q('[data-reviewed]').checked=false;status(e.message,true);}
    finally{busy=false;allowPublish();}
  };
  return { refresh() {
    if(draft && getState()?.generation !== draft.generation) {
      draft=null;q('[data-reviewed]').checked=false;allowPublish();
      status('Exercise changed after this draft. Compute and review a fresh forecast.',true);
    }
  }};
}
