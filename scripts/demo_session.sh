#!/usr/bin/env bash
# Drive a two-turn demo session through the HTTP API (the same calls the web
# UIs make). Requires a running `ttx` (default http://localhost:8080) with the
# LLM configured. Screenshots of the simulator window go to runs/demo-screenshots/.
#
#   ./scripts/demo_session.sh [base_url]
set -euo pipefail
B="${1:-http://localhost:8080}/api"
H='content-type: application/json'
: "${TTX_MASTER_KEY:?Set TTX_MASTER_KEY to the EXCON access key printed at startup}"
A="Authorization: Bearer $TTX_MASTER_KEY"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
post() { curl -sf -H "$A" -XPOST "$B$1" -H "$H" ${2:+-d "$2"}; echo; }
reply() {
  local role="$1" parent body
  parent=$(curl -sf -H "$A" "$B/player/$role" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(next(m["id"] for m in reversed(d["inbox"]) if m["kind"] in ("inject","clarification") and m["to"] == d["player"]["role"]))')
  body=$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); d["reply_to"]=int(sys.argv[2]); d["evidence"]={"basis":{"kind":"operational_judgment","reference":"Operational judgment based on the delivered situation and available units"},"attachments":[]}; print(json.dumps(d))' "$2" "$parent")
  post "/player/$role/reply" "$body"
}
wait_parsed() { # wait until no proposal is still being parsed
  for _ in $(seq 60); do
    curl -sf -H "$A" "$B/master" | grep -q '"status":"parsing"' || return 0; sleep 1
  done
}
last_pid() { curl -sf -H "$A" "$B/master" | python3 -c 'import json,sys; print(json.load(sys.stdin)["proposals"][-1]["id"])'; }
shot() { mkdir -p "$ROOT/runs/demo-screenshots"; echo "$ROOT/runs/demo-screenshots/$1" > "$ROOT/runs/shot.req"; sleep 2; }

echo "== setup: regional player plays in Armenian"
post /player/regional_cp/language '{"language":"hy"}'
post /player/mayor/language '{"language":"it"}'
post /player/fire_commander/language '{"language":"en"}'
post /master/start
for i in I-01 I-02 I-03; do post /master/msel/$i/send; done

echo "== turn 1: players reply in their own words"
reply mayor "{\"text\":\"Diramate subito l'allerta alla popolazione della zona F4 ed evacuate la scuola primaria di Pineta verso il centro sportivo di Valverde, che va aperto come area di accoglienza. Chiudete la Northern Road al traffico civile. Informate il comandante dei vigili del fuoco che la scuola sarà evacuata con i nostri autobus.\"}"
reply fire_commander '{"text":"Two crews cut a fire line along the northern edge of zone F4 to protect Pineta; the third crew cuts from Pineta east edge to Forest edge east. Air tanker: one drop on Pineta north edge. Tell regional civil protection we may need reinforcements soon."}'
wait_parsed

echo "== EXCON prompt: one more bus for the mayor"
post /master/prompt '{"text":"The regional transport company makes one extra bus available to the mayor right now. Send the mayor a short inject from the Regional transport company confirming it."}'
wait_parsed
for id in $(curl -sf -H "$A" "$B/master" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(" ".join(str(p["id"]) for p in sorted(d["proposals"], key=lambda p: p["source"]["kind"]!="master") if p["status"]=="pending"))'); do
  post /master/proposals/$id/decide '{"approve":true}'
done
sleep 5
echo "== resolve turn 1 (30 min)"
post /master/resolve '{"minutes":30}' > /dev/null
sleep 3; shot sim_turn1_end.png

echo "== EXCON confirms first-turn fire tasks completed and units returned"
for body in $(curl -sf -H "$A" "$B/state" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(" ".join(json.dumps({"type":"ReturnResource","allocation":a["id"],"qty":a["qty"]-a["returned"]},separators=(",",":")) for a in d["world"]["allocations"] if a["role"]=="fire_commander" and a["qty"]>a["returned"]))'); do
  post /master/command "$body"
done
echo "== turn 2: escalation"
post /master/msel/I-04/send
post /master/command '[{"type":"SetWeather","wind_speed_kmh":55},{"type":"Ignite","at":"G5","radius_m":60}]'
reply regional_cp '{"text":"Խնդրում ենք միջազգային օգնությամբ (UCPM) 2 լրացուցիչ օդային տանկեր և տարածաշրջանային հրշեջ ծառայությունից 2 վերգետնյա թիմ՝ հրշեջ հրամանատարի համար։ Տեղեկացրեք քաղաքապետին, որ Valverde Sports Centre-ը պատրաստ է ընդունել տարհանվածներին։"}'
reply fire_commander '{"text":"Send the remaining crews over there to hold it."}'
wait_parsed
post /master/proposals/$(last_pid)/decide '{"approve":true,"note":"Send the reviewed clarification asking for the zone edge."}'
reply fire_commander '{"text":"One crew along the east edge of zone F4, the other along the north edge of G4. Air tanker drop on G4."}'
wait_parsed
for id in $(curl -sf -H "$A" "$B/master" | python3 -c 'import json,sys; print(" ".join(str(p["id"]) for p in json.load(sys.stdin)["proposals"] if p["status"]=="pending"))'); do
  post /master/proposals/$id/decide '{"approve":true}'
done
sleep 5
echo "== resolve turn 2 (60 min)"
post /master/resolve '{"minutes":60}' > /dev/null
sleep 3; shot sim_turn2_end.png
curl -sf -H "$A" "$B/state" | python3 -c 'import json,sys; print(json.load(sys.stdin)["world"]["stats"])'
