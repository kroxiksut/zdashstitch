# Brand artwork provenance

This directory holds the in-house ZDashstitch artwork: the logo, the application icon, the Stezhik (Стежик) mascot and the README illustrations. It is original project artwork and is **not** part of Tabler Icons; third-party interface icons are recorded in [`../icons/THIRD_PARTY.md`](../icons/THIRD_PARTY.md).

## Author and licence

- **Author:** the project owner, who wrote the specifications and selected the results (confirmed by the owner on 2026-10-25 and 2026-10-26). The images were generated with ChatGPT; see «How it was made».
- **Licence:** [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) (documentation and original visuals; see `LICENSING.md` and `LICENSE-DOCS`). Attribution: ZDashstitch project, with a link to the licence.
- The licence does not extend to Zabbix names or trademarks; no Zabbix artwork is included.

## Files

Size is in bytes. The hash is the beginning of the SHA-256 of the file as of 2026-10-26, so that a later replacement of a file is noticed and this table is updated together with it.

| File (under `assets/brand/`) | Pixels | Size | SHA-256 | Purpose |
|---|---|---|---|---|
| `banners/readme-en.png` | 2172x724 | 1717138 | `c8b58def81b4d7b9…` | README banner (English). Visual direction concept art with text; used in `README.md`; not for the application UI. |
| `banners/readme-ru.png` | 1774x887 | 1755392 | `388c4fc8608fb00b…` | README banner (Russian). Same role; used in `README.ru.md`; not for the application UI. |
| `icons/stezhik-head-1024.png` | 1024x1024 | 734605 | `5e5a47c3184afa67…` | Stezhik head: application logo in the header and in «About and licenses» (P1-12); also the base for application icons. |
| `icons/zdashstitch.ico` | 7 sizes, 16-256 px | 104887 | `95ff339e6c79103c…` | Application icon for the executable/installer (P6-01). |
| `mascot/stezhik-wave.png` | 1254x1254 | 1268563 | `5ae85cddc148e9e2…` | Stezhik waving: welcome, empty drafts (P1-10). |
| `mascot/stezhik-chart.png` | 1254x1254 | 1246945 | `bd90acf6b08d1d87…` | Stezhik with a chart: long safe operation, empty library (P1-10). |
| `mascot/stezhik-compare.png` | 1254x1254 | 1379780 | `38be9bf44294cd17…` | Stezhik with two cards and arrows: empty bindings (P1-10). |
| `mascot/stezhik-inspect.png` | 1254x1254 | 1431499 | `10de7af3e7486b76…` | Stezhik with a magnifier: «no findings» state (P1-10). |
| `mascot/stezhik-success.png` | 1254x1254 | 1292661 | `cbeae60835978f5e…` | Stezhik with a thumbs up: only after a verified result (P1-10). |
| `overview/product-vision-en.png` | 1536x1024 | 2045520 | `05ec10e1b1f30c09…` | Product vision illustration (English); concept art, not a product screenshot; not for the application UI. |

Usage rules for the application UI (which images may and may not be shown) are in `docs/design/P1-10-motion.md`; the banners and the overview image are forbidden there.

## How it was made / source files

Recorded from the owner's statement of 2026-10-26.

- **Creation method:** every file in this directory was generated with ChatGPT (OpenAI) from technical specifications written by the project owner. The human contribution is the specification, iteration and selection of the results; the files are the model outputs as selected by the owner.
- **Tools and versions:** ChatGPT image generation; the exact model version was not recorded.
- **Creation dates:** on or before 2026-10-23 (file timestamps); first committed on 2026-10-24.
- **Editable sources:** none; the raster files listed above are the originals. There are no vector or layered sources.
- **Third-party material:** no fonts, stock images or other third-party files were added to the images by hand.
- **Tool terms and the licence:** OpenAI's terms of use assign the user the rights in the output to the extent the law allows. In some jurisdictions purely machine-generated images may receive limited or no copyright protection; the CC-BY-4.0 grant above applies to whatever rights the project holds in these files. Re-check the provider's terms before each release if they change.

## Display in the application

The in-app «About and licenses» window (P1-12) must show this notice (author, licence CC-BY-4.0 and the creation record above) together with the other licence texts.
