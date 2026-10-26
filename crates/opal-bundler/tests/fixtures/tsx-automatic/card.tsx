export function Card({ title, children }: { title: string; children?: unknown }) {
  return <section data-title={title}>{children ?? null}</section>;
}
