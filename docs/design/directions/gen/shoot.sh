#!/bin/sh
cd "$(dirname "$0")/.."
for d in ${DIRS:-a b c}; do
  for p in overview payments payment; do
    google-chrome --headless=new --no-sandbox --disable-gpu --hide-scrollbars --force-device-scale-factor=1 --window-size=1440,900 --screenshot="$d-$p-1440.png" "file://$PWD/$d/$p.html" >/dev/null 2>&1
    google-chrome --headless=new --no-sandbox --disable-gpu --hide-scrollbars --force-device-scale-factor=1 --window-size=390,844 --screenshot="$d-$p-390.png" "file://$PWD/$d/$p.html" >/dev/null 2>&1
  done
done
ls *.png
