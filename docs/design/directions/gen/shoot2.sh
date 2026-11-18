#!/bin/sh
cd "$(dirname "$0")/../round2"
for d in ${DIRS:-d e f}; do
  for p in overview payments payment; do
    for s in 1440,900 390,844; do
      w=${s%,*}
      google-chrome --headless=new --no-sandbox --disable-gpu --hide-scrollbars --force-device-scale-factor=1 --window-size=$s --screenshot="$d-$p-$w.png" "file://$PWD/$d/$p.html" >/dev/null 2>&1
    done
  done
done
ls *.png | wc -l
