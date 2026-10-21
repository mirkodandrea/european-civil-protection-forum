import {api, esc} from './common.js';

// The assistant's questions stay here until the player confirms the final message.
export function playerPreparation(container, textInput, sendButton, interpret, getState, refresh, setText) {
  const drafts = new Map(), dismissed = new Set();
  let selected, busy = false, error = '';
  function current() { return drafts.get(selected); }
  function render() {
    const record = current(), draft = record?.draft;
    const active = interpret.checked && getState()?.phase === 'orders';
    container.hidden = !active || (!draft && !busy && !error);
    sendButton.textContent = !active ? 'Invia →' : busy ? 'Analisi…' : draft?.status === 'needs_clarification' ? 'Chiarisci →' : draft?.status === 'ready' ? 'Analizza modifiche' : 'Prepara decisione';
    textInput.placeholder = active && draft?.status === 'needs_clarification' ? 'Rispondi alla domanda dell’assistente…' : 'Scrivi un messaggio…';
    sendButton.disabled = busy || !getState()?.player.language_confirmed || (active && draft?.status === 'ready' && !record.dirty);
    if (!active) return;
    if (busy) { container.innerHTML = '<p class="muted" role="status">L’assistente sta leggendo la tua bozza. Nessun messaggio è stato inviato.</p>'; return; }
    if (!draft) { container.innerHTML = error ? `<p class="error">${esc(error)}</p>` : ''; return; }
    const clarify = draft.status === 'needs_clarification';
    const unavailable = draft.status === 'ai_unavailable';
    container.innerHTML = `<div class="private-draft-head"><b>${clarify ? 'L’assistente chiede a te' : unavailable ? 'Assistente non disponibile' : 'Controlla prima di inviare'}</b><span class="pill">Bozza privata · non inviata</span></div>
      ${clarify ? `<p class="draft-question">${esc(draft.question)}</p><p class="muted">Rispondi nel campo qui sotto. EXCON e il destinatario non hanno ancora ricevuto questo messaggio.</p>` : unavailable ? '<p>L’AI non ha potuto interpretare la bozza. Puoi riprovare oppure chiedere esplicitamente a EXCON di interpretarla.</p>' : `<p class="draft-message">${esc(draft.message)}</p><p class="muted">${draft.actions.length ? 'Conferma esplicitamente le azioni qui sotto: saranno inviate a EXCON per la revisione.' : 'Messaggio informativo · nessuna azione sul simulatore'}</p>${draft.action_descriptions.map(description=>`<p class="draft-action">${esc(description)}</p>`).join('')}`}
      ${record.dirty ? '<p class="muted">Hai modificato il testo: analizzalo di nuovo prima di confermare.</p>' : ''}
      ${error ? `<p class="error">${esc(error)}</p>` : ''}
      <div class="row">${!clarify ? `<button id="confirm-prepared" class="primary" type="button" ${record.dirty?'disabled':''}>${unavailable ? 'Invia a EXCON per interpretazione manuale' : draft.actions.length ? draft.to === 'master' ? 'Conferma azioni e invia a EXCON' : 'Conferma · invia chat al player e azioni a EXCON' : draft.to === 'master' ? 'Conferma e invia a EXCON' : 'Conferma e invia al destinatario'}</button>` : ''}<button id="discard-prepared" type="button">Scarta bozza</button></div>`;
    container.querySelector('#discard-prepared').onclick = () => { clear(selected); setText(selected,''); render(); };
    const confirm = container.querySelector('#confirm-prepared');
    if (confirm) confirm.onclick = () => submit(selected, record);
  }
  textInput.addEventListener('input', () => {
    const record = current();
    if (record && record.draft.status !== 'needs_clarification') { record.dirty = textInput.value !== record.draft.message; render(); }
  });
  interpret.addEventListener('change', render);

  async function prepare(to, text) {
    if (busy || !text.trim()) return;
    busy = true; error = '';
    const record = drafts.get(to);
    const previous = record?.draft.status === 'needs_clarification' && !record.dirty ? record.draft : null;
    sendButton.disabled = true;
    render();
    try {
      const result = await api(`/api/player/${getState().player.role}/draft`, {to, text, ...(previous ? {draft:previous.id, revision:previous.revision} : {})});
      drafts.set(to, {draft:result.draft, dirty:false, clockRevision:getState().clock_control.revision});
      setText(to, result.draft.status === 'needs_clarification' ? '' : result.draft.message);
    } catch (failure) { error = failure.message; }
    finally { busy = false; sendButton.disabled = false; render(); }
  }
  async function submit(to, record) {
    if (busy || record.dirty) return;
    busy = true; error = ''; sendButton.disabled = true; render();
    try {
      await api(`/api/player/${getState().player.role}/draft/submit`, {draft:record.draft.id, revision:record.draft.revision, manual:record.draft.status === 'ai_unavailable'});
      clear(to); setText(to,'');
      await refresh();
    } catch (failure) { error = failure.message; }
    finally { busy = false; sendButton.disabled = false; render(); }
  }
  function clear(to) { const record=drafts.get(to);if(record)dismissed.add(record.draft.id);drafts.delete(to);error=''; }
  function update(state, to) {
    selected = to;
    for (const draft of state.drafts || []) {
      if (!dismissed.has(draft.id) && !drafts.has(draft.to) && draft.status !== 'interpreting') {
        drafts.set(draft.to, {draft,dirty:false,clockRevision:state.clock_control.revision});
        setText(draft.to, draft.status === 'needs_clarification' ? '' : draft.message);
      }
    }
    // A turn change/rollback invalidates the private interpretation.
    for (const [role,record] of drafts) if((record.draft.turn!==state.turn || record.clockRevision!==state.clock_control.revision) && !busy) drafts.delete(role);
    render();
  }
  return {prepare, update, clear, isBusy:()=>busy};
}
