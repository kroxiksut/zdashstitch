# Third-party interface icons

This directory records the provenance and licence of icon artwork used by ZDashstitch. It does not cover the in-house ZDashstitch logo or Stezhik artwork under `assets/brand/`.

## Tabler Icons (all platforms)

- **Author:** Tabler / Paweł Kuna and contributors
- **Licence:** MIT
- **Home page:** <https://tabler.io/icons>
- **Source:** <https://github.com/tabler/tabler-icons>
- **Style used by ZDashstitch:** outline
- **Scope:** interface, status and tray icons on every supported platform

ZDashstitch contributors must not invent, redraw or silently substitute interface, status or tray icons. Select an existing icon from the Tabler outline set, record its upstream name, and preserve the accessible label/message ID separately from the artwork. The application's own logo is original project artwork and is not part of Tabler Icons.

Agents may download the Tabler outline SVGs required by their current task directly from the official `tabler/tabler-icons` upstream without additional approval. They must use an exact release tag or commit rather than an unpinned `latest` URL, vendor only the required files, preserve upstream filenames/names, validate that the SVGs contain no scripts or external references, and keep runtime use local/offline and CSP-safe. This authorization does not cover another icon library or unrelated third-party assets.

When actual Tabler SVGs or a Tabler package are added to a distributable artifact, record the pinned upstream package/release or commit, source URLs and included icon names here or in a generated inventory next to this notice. This notice and the MIT text below must travel with the corresponding source and binary distributions and must be exposed by the in-app Licenses window.

### MIT License

Copyright (c) 2020-2026 Paweł Kuna

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
