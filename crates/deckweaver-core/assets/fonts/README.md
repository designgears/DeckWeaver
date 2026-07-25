# Bundled fonts

`Inter-Bold.ttf` is embedded into `deckweaver-core` with `include_bytes!` (see
`src/render/text.rs`). Bundling rather than probing system font paths keeps the rendered strip
pixel-identical on every distro; the previous probe silently drew no text at all when none of
the hard-coded paths existed.

Only Bold is bundled. Everything on the encoder strip is 12–16px on a physically small LCD and
has to stay readable over whatever background the user set; lighter weights render thin and
wash out. `src/render/text.rs` additionally boosts glyph coverage after downsampling, which
compensates for the softness an unhinted supersampled raster leaves behind. ExtraBold was
tried and rejected — it closes up the counters on the 12px chip labels.

Inter is licensed under the SIL Open Font License 1.1 — see `LICENSE.txt`.

## Regenerating

The checked-in file is a **subset** of the upstream static TTF, cut from ~420 KB to ~125 KB
because the compiled artifacts are committed to this repo on every release. The subset keeps
Latin, Latin Extended, Greek, Cyrillic, Vietnamese, general punctuation (including the `…` used
for name truncation) and currency symbols.

```sh
curl -sSLO https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip
unzip -o Inter-4.1.zip 'extras/ttf/Inter-Bold.ttf' LICENSE.txt

pyftsubset extras/ttf/Inter-Bold.ttf \
  --output-file=Inter-Bold.ttf \
  --unicodes="U+0000-024F,U+0259,U+0370-03FF,U+0400-04FF,U+1E00-1EFF,U+2000-206F,U+20A0-20BF,U+2122,U+2126,U+FFFD" \
  --layout-features="kern,liga,calt" \
  --no-hinting --desubroutinize --name-IDs="*" --notdef-outline
```
