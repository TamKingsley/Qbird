// Splits plain chat text into text nodes and anchors. Nothing from the message is
// inserted as HTML: every piece becomes a text node, so a crafted message cannot
// inject markup into the app origin.
const URL_PATTERN = /(?:https?:\/\/|www\.)[^\s<>"']+/gi;
const TRAILING = /[),.;:!?，。；：！？、）】》」』]+$/;

export function linkifiedText(text: string): DocumentFragment {
  const fragment = document.createDocumentFragment();
  let index = 0;
  URL_PATTERN.lastIndex = 0;
  for (let match = URL_PATTERN.exec(text); match; match = URL_PATTERN.exec(text)) {
    const raw = match[0].replace(TRAILING, '');
    if (!raw) continue;
    if (match.index > index) fragment.append(document.createTextNode(text.slice(index, match.index)));
    // A button keeps the existing delegated click handler (it only looks at
    // button[data-action]) and avoids a real navigation in the app origin.
    const anchor = document.createElement('button');
    anchor.type = 'button';
    anchor.className = 'message-link';
    anchor.dataset.action = 'open-url';
    anchor.dataset.url = /^https?:\/\//i.test(raw) ? raw : `https://${raw}`;
    anchor.textContent = raw;
    anchor.title = '在右侧工作区打开';
    fragment.append(anchor);
    index = match.index + raw.length;
  }
  if (index < text.length) fragment.append(document.createTextNode(text.slice(index)));
  return fragment;
}

/// Replaces the element content with linkified text; keeps callers from touching
/// innerHTML directly.
export function paintLinkified(element: HTMLElement, text: string): void {
  element.replaceChildren(linkifiedText(text));
}
