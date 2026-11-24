#!/usr/bin/env bash
# Makes the test audio for one language with espeak-ng (a synthetic voice, no
# person's recording) as 16 kHz mono 16-bit WAV, what Sokki expects:
#   <out>/<lang>/keyboard.wav + .txt   what the Sokki keyboard mic "hears"
#   <out>/<lang>/mic.wav + .txt        what plays into the phone's microphone
# Every sentence is made up. Usage: bash scripts/make_audio.sh <out> <lang>
# Needs espeak-ng and sox (CI: apt-get install espeak-ng sox).
set -euo pipefail
OUT=$1 LANG_=$2
case "$LANG_" in
  de) KB="Hallo Maria, ich komme heute zehn Minuten später. Fangt bitte ohne mich an."
      MIC="Das ist ein Test für Sokki. Wir planen das Sommerfest am Freitag um drei Uhr im Garten." ;;
  en) KB="Hi Sam, I am running ten minutes late. Please start without me."
      MIC="This is a test for Sokki. We are planning the summer party for Friday at three in the garden." ;;
  fr) KB="Salut Marie, j'ai dix minutes de retard. Commencez sans moi, s'il vous plaît."
      MIC="Ceci est un test pour Sokki. Nous préparons la fête d'été pour vendredi à quinze heures dans le jardin." ;;
  es) KB="Hola Marta, llego diez minutos tarde. Empezad sin mí, por favor."
      MIC="Esto es una prueba para Sokki. Preparamos la fiesta de verano para el viernes a las tres en el jardín." ;;
  pl) KB="Cześć Marta, spóźnię się dziesięć minut. Zacznijcie beze mnie, proszę."
      MIC="To jest test aplikacji Sokki. Planujemy letnie przyjęcie w piątek o trzeciej w ogrodzie." ;;
  *) echo "no test sentences for language $LANG_" >&2; exit 1 ;;
esac
mkdir -p "$OUT/$LANG_"
say() {  # text, name: speak slowly, then 16 kHz mono with a second of silence at the end
  espeak-ng -v "$LANG_" -s 140 -w "$OUT/$LANG_/$2.raw.wav" "$1"
  sox "$OUT/$LANG_/$2.raw.wav" -r 16000 -c 1 -b 16 "$OUT/$LANG_/$2.wav" gain -n -3 pad 0.5 1.5
  rm "$OUT/$LANG_/$2.raw.wav"
  printf '%s\n' "$1" > "$OUT/$LANG_/$2.txt"
}
say "$KB" keyboard
say "$MIC" mic
echo "test audio for $LANG_ in $OUT/$LANG_"
