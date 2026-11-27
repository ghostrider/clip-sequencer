#!/usr/bin/env bash
# M21 probe scenes on layer 2 ("wqewe"): clip 1 Gradient, clip 2 Sine Wave. Usage: scene.sh <n>
# The param ids below are those of the probe composition ("Untitled", Arena 7.29.0, 2026-11-27): take yours
# from `resolume-probe dump` (layer 2 transition.duration / blend_mode, clips 1 and 2 connected, clip 2 transition).
# Each scene starts with layer 2 at 0 s / Alpha and clip 1 connected, and puts the layer back to 0 s / Alpha.
D=1795710392780; B=1795710392688
S="${OUT:-docs/samples}"   # where the logs go (m21-s<n>.jsonl)
dur(){ echo "ws {\"action\":\"set\",\"parameter\":\"/parameter/by-id/$D\",\"value\":$1}"; }
blend(){ echo "ws {\"action\":\"set\",\"parameter\":\"/parameter/by-id/$B\",\"value\":\"$1\"}"; }
con(){ echo "osc /composition/layers/2/clips/$1/connect i:1"; }
start=( "sub /parameter/by-id/1795710390922" "sub /parameter/by-id/1795710390762" "sub /parameter/by-id/$D" "sub /parameter/by-id/$B" "wait 300" "$(dur 0)" "$(blend Alpha)" "wait 300" "$(con 1)" "until Connected 1000" "wait 1500" )
end=( "wait 1000" "$(dur 0)" "$(blend Alpha)" )
case "$1" in
  1) steps=( "mark S1 set 2 s Dissolve 1 s ahead, then Sine Wave" "$(dur 2)" "$(blend Dissolve)" "wait 1000" "$(con 2)" "wait 3000" ) ;;
  2) steps=( "mark S2 set 2 s Dissolve in the same instant as the connect" "$(dur 2)" "$(blend Dissolve)" "$(con 2)" "wait 3000" ) ;;
  3) steps=( "mark S3 set 2 s Dissolve 30 ms ahead" "$(dur 2)" "$(blend Dissolve)" "wait 30" "$(con 2)" "wait 3000" ) ;;
  4) steps=( "mark S4 4 s fade, set to 0 after 1 s" "$(dur 4)" "$(blend Dissolve)" "wait 1000" "$(con 2)" "wait 1000" "$(dur 0)" "wait 4000" ) ;;
  5) steps=( "mark S5 3 s Dissolve, then clear the layer" "$(dur 3)" "$(blend Dissolve)" "wait 1000" "osc /composition/layers/2/clear i:1" "osc /composition/layers/2/clear i:0" "wait 4000" ) ;;
  6) steps=( "mark S6 3 s fade, blend changed to Wipe Ellipse after 1 s" "$(dur 3)" "$(blend Dissolve)" "wait 1000" "$(con 2)" "wait 1000" "$(blend 'Wipe Ellipse')" "wait 3000" ) ;;
  7) steps=( "mark S7 4 s fade, set 8 s after 1 s" "$(dur 4)" "$(blend Dissolve)" "wait 1000" "$(con 2)" "wait 1000" "$(dur 8)" "wait 9000" ) ;;
  8) steps=( "mark S8 4 s fade, set 2 s after 1 s" "$(dur 4)" "$(blend Dissolve)" "wait 1000" "$(con 2)" "wait 1000" "$(dur 2)" "wait 5000" ) ;;
  9) steps=( "mark S9 2 s fade, next connect during the fade" "$(dur 2)" "$(blend Dissolve)" "wait 1000" "$(con 2)" "wait 1000" "$(con 1)" "wait 4000" ) ;;
  10) steps=( "sub /parameter/by-id/1795710390847" "sub /parameter/by-id/1795710390851" "sub /parameter/by-id/1795710390888" "wait 300" "mark S10 clip 2 layer_determined false, clip duration 0; layer 2 s" 'ws {"action":"set","parameter":"/parameter/by-id/1795710390847","value":false}' "$(dur 2)" "$(blend Dissolve)" "wait 1000" "$(con 2)" "wait 3000" 'ws {"action":"set","parameter":"/parameter/by-id/1795710390847","value":true}' ) ;;
  11) steps=( "sub /parameter/by-id/1795710390847" "sub /parameter/by-id/1795710390851" "sub /parameter/by-id/1795710390888" "wait 300" "mark S11 clip 2 duration 1 s (layer_determined left as is); layer 3 s" 'ws {"action":"set","parameter":"/parameter/by-id/1795710390851","value":1}' "$(dur 3)" "$(blend Dissolve)" "wait 1000" "$(con 2)" "wait 4000" 'ws {"action":"set","parameter":"/parameter/by-id/1795710390851","value":0}' 'ws {"action":"set","parameter":"/parameter/by-id/1795710390888","value":"Layer Determined"}' 'ws {"action":"set","parameter":"/parameter/by-id/1795710390847","value":true}' ) ;;
  *) echo "scene 1..6"; exit 1 ;;
esac
cd "$(dirname "$0")/../../.."
cargo run -q -p resolume-probe -- --out "$S" session --log "m21-s$1" --tail-ms 500 "${start[@]}" "${steps[@]}" "${end[@]}" 2>&1 \
  | grep -E -- '----|until|error|Connected|Disconnected' | grep -v -E 'recv BIG' | cut -c1-200
