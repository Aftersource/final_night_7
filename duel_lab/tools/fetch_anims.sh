#!/bin/bash
# Fetch each Meshy clip as GLB (free re-download), strip it to skeleton + animation for the web. Re-runnable.
U=~/Desktop/source/aftersource/final_night_7_unreal
set -a; . $U/__production_steps/meshy.env; set +a
HERE=$(cd "$(dirname "$0")" && pwd); LAB=$HERE/..; RAW=$LAB/raw; P=$U/production_assets/meshy/20261114_152015_mannequin_01a1dbb7
F="--output-schema v1 --format json --no-update-check"
one(){ n=$1; tj=$2; key=$3
  [ -f $LAB/assets/anims/$n.glb ] && return
  [ -f "$tj" ] || return
  meshy download --task-json "$tj" --asset $key --output "$RAW/$n.glb" --workspace "$RAW" $F >/dev/null 2>&1
  [ -f $RAW/$n.glb ] && (cd $HERE && node convert.mjs anim $RAW/$n.glb $LAB/assets/anims/$n.glb $n) && rm -f $RAW/$n.glb; }
RIG=$P/task_01a1dbb9-fcfd-719d-9b0a-28abc0cd1af4.json
one walk $RIG result.basic_animations.walking_glb_url
one run $RIG result.basic_animations.running_glb_url
while read n _ _ _ t; do one $n $P/task_$t.json result.animation_glb_url; done < $U/__production_steps/meshy_jobs.log
ls $LAB/assets/anims | wc -l; du -sh $LAB/assets/anims
