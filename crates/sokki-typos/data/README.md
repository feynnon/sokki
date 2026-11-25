# Word lists for the Mac typo model

`lexicon-de.txt` and `lexicon-en.txt`: every word of the German (legacy) and US English
dictionaries the phone keyboard (HeliBoard, from AOSP) uses, one per line, as the dictionary
spells it. On the phone the keyboard's dictionaries tell autocorrect whether a typed word is a
word and how it is spelled (Montag, Straße); on the Mac these lists do. Made by
`tools/gen_lexicon.py` from the dictionaries' `.combined` source files.

The word and pair tables (`words.bin`, `bigrams.bin`) are the phone's own, in
`mobile/keyboard/src/main/assets/sokki/`.
