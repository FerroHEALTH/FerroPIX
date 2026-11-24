// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1
//
// Draws every ```mermaid fence of the page with the vendored mermaid library.
// The page's policy refuses inline styles, so the CSS mermaid writes into each
// diagram is applied through a constructed stylesheet, which the policy allows,
// and a diagram is laid out only once that sheet is in place.
// No specification governs this: our own design.
(async () => {
  const fences = [...document.querySelectorAll('pre > code.language-mermaid')];
  if (fences.length === 0) {
    return;
  }
  const darkThemes = ['ayu', 'navy', 'coal'];
  const html = document.documentElement;
  const isDark = () => darkThemes.some((theme) => html.classList.contains(theme));
  const drawnDark = isDark();
  mermaid.initialize({
    startOnLoad: false,
    theme: drawnDark ? 'dark' : 'default',
    htmlLabels: false,
    flowchart: { htmlLabels: false },
  });
  const sheet = new CSSStyleSheet();
  document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  const sources = fences.map((code) => code.textContent);
  let css = '';
  for (const [i, source] of sources.entries()) {
    const { svg } = await mermaid.render(`mermaid-probe-${i}`, source);
    for (const match of svg.matchAll(/<style>([\s\S]*?)<\/style>/g)) {
      css += match[1].replaceAll(`#mermaid-probe-${i}`, `#mermaid-${i}`);
    }
  }
  sheet.replaceSync(css);
  for (const [i, source] of sources.entries()) {
    const { svg } = await mermaid.render(`mermaid-${i}`, source);
    const figure = document.createElement('div');
    figure.className = 'mermaid';
    figure.innerHTML = svg;
    fences[i].parentElement.replaceWith(figure);
  }
  new MutationObserver(() => {
    if (isDark() !== drawnDark) {
      window.location.reload();
    }
  }).observe(html, { attributes: true, attributeFilter: ['class'] });
})();
