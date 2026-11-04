/** `el('div', { class: 'card' }, child, 'text')`: enough DOM building for a theme this size. */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Record<string, string> = {},
  ...children: (Node | string | null)[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [name, value] of Object.entries(props)) node.setAttribute(name, value);
  for (const child of children) if (child !== null) node.append(child);
  return node;
}

/** A theme's logo where its name would be; the name itself if the picture cannot load. */
export function themeLogo(src: string, name: string): HTMLImageElement {
  const img = el('img', { class: 'theme-logo', src, alt: name });
  img.addEventListener('error', () => img.replaceWith(name));
  return img;
}
