import {esc} from './common.js';

// Inject authoring is a mode of the conversation's existing message composer.
export function conversationInjects(container, roles, templates, textInput, sendButton) {
  let role, state, signature = '';
  const drafts = new Map();
  const roleName = id => roles.find(item => item.id === id)?.name || id;
  container.innerHTML = `
    <div class="row conversation-message-kind">
      <label for="message-kind">Scrivi come</label>
      <select id="message-kind"><option value="chat">EXCON · messaggio</option><option value="inject">Inject · evento dell’esercitazione</option></select>
    </div>
    <div id="inject-options" hidden>
      <div class="inject-compose-row"><label for="inject-template">Da scenario</label>
        <select id="inject-template"></select></div>
      <div class="inject-compose-grid">
        <label>Titolo<input id="inject-title" maxlength="200" placeholder="Es. Segnalazione dal territorio"></label>
        <label>Mittente simulato<input id="inject-sender" maxlength="200" value="EXCON" placeholder="Es. 112 Dispatch"></label>
      </div>
      <div class="inject-compose-row"><label for="inject-scope">Destinatari</label><select id="inject-scope"></select></div>
      <p id="inject-context" class="muted"></p>
    </div>`;
  const $ = id => container.querySelector(`#${id}`);

  function template() {
    return templates.find(item => item.id === $('inject-template').value);
  }
  function scopes(saved = 'conversation') {
    const item = template();
    const audience = item?.recipients.includes('all') ? 'Tutti i player' : item?.recipients.map(roleName).join(', ');
    $('inject-scope').innerHTML = `<option value="conversation">Solo ${esc(roleName(role))} · questa conversazione</option>` +
      (item ? `<option value="scenario">${esc(audience)} · destinatari dello scenario</option>` : '');
    $('inject-scope').value = item && saved === 'scenario' ? 'scenario' : 'conversation';
  }
  function render() {
    const isInject = $('message-kind').value === 'inject';
    $('inject-options').hidden = !isInject;
    $('inject-title').required = isInject;
    $('inject-sender').required = isInject;
    const item = template();
    $('inject-context').textContent = item ? `${item.id} · previsto turno ${item.turn}${state?.sent_injects.includes(item.id) ? ' · già inviato, questa è una nuova copia' : ''}${item.objective ? ` · ${item.objective}` : ''}` : 'L’inject verrà tradotto e apparirà qui come messaggio. Il player risponde scrivendo in chat.';
    sendButton.textContent = isInject ? 'Invia inject →' : 'Invia →';
  }
  $('message-kind').onchange = render;
  $('inject-template').onchange = () => {
    const item = template();
    $('inject-title').value = item?.title || '';
    $('inject-sender').value = item?.sender || 'EXCON';
    textInput.value = item?.text || '';
    textInput.dispatchEvent(new Event('input'));
    scopes();
    render();
  };

  function update(nextState, nextRole) {
    state = nextState;
    const changedRole = nextRole !== role;
    if (changedRole && role) {
      drafts.set(role, {
        kind:$('message-kind').value, template:$('inject-template').value,
        title:$('inject-title').value, sender:$('inject-sender').value, scope:$('inject-scope').value,
      });
    }
    role = nextRole;
    const key = JSON.stringify([role, state.sent_injects]);
    if (key !== signature) {
      signature = key;
      const chosen = changedRole ? drafts.get(role)?.template : $('inject-template').value;
      $('inject-template').innerHTML = '<option value="">Inject personalizzato</option>' + templates
        .filter(item => item.recipients.includes('all') || item.recipients.includes(role))
        .map(item => `<option value="${esc(item.id)}">${esc(item.id)} · ${esc(item.title)} · turno ${item.turn}${state.sent_injects.includes(item.id) ? ' · inviato' : ''}</option>`).join('');
      $('inject-template').value = chosen || '';
    }
    if (changedRole) {
      const saved = drafts.get(role);
      $('message-kind').value = saved?.kind || 'chat';
      $('inject-title').value = saved?.title || '';
      $('inject-sender').value = saved?.sender || 'EXCON';
      scopes(saved?.scope);
    } else {
      scopes($('inject-scope').value);
    }
    render();
  }
  function request() {
    if ($('message-kind').value !== 'inject') return null;
    const item = template();
    const title = $('inject-title').value.trim();
    const sender = $('inject-sender').value.trim();
    const text = textInput.value.trim();
    if (!title || !sender || !text) throw Error('Compila titolo, mittente e messaggio dell’inject.');
    const scenario = item && $('inject-scope').value === 'scenario';
    if (scenario && !state.sent_injects.includes(item.id) && title === item.title && sender === item.sender && text === item.text) {
      return {path:`/api/master/msel/${encodeURIComponent(item.id)}/send`, body:{}};
    }
    return {path:'/api/master/inject', body:{title, sender, text, recipients:scenario ? item.recipients : [role]}};
  }
  function clear(sentRole) {
    drafts.delete(sentRole);
    if (role !== sentRole) return;
    $('inject-template').value = '';
    $('inject-title').value = '';
    $('inject-sender').value = 'EXCON';
    $('message-kind').value = 'chat';
    scopes();
    render();
  }
  return {update, request, clear};
}
