import { getCollection, type CollectionEntry } from "astro:content";

export type Post = CollectionEntry<"posts">;

/** Newest first. Two releases can share a day, so the version breaks the tie. */
export async function postsNewestFirst(): Promise<Post[]> {
  const posts = await getCollection("posts");
  return posts.sort(
    (a, b) =>
      b.data.date.getTime() - a.data.date.getTime() ||
      b.id.localeCompare(a.id, "en", { numeric: true }),
  );
}

/** Backticks to <code>, for the one-line ledes and summaries kept in frontmatter. */
export function inlineCode(text: string): string {
  const escaped = text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return escaped.replace(/`([^`]+)`/g, "<code>$1</code>");
}

const long = new Intl.DateTimeFormat("en-US", { month: "long", day: "numeric", year: "numeric", timeZone: "UTC" });
const short = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });

export const longDate = (date: Date) => long.format(date);
export const shortDate = (date: Date) => short.format(date);
export const isoDay = (date: Date) => date.toISOString().slice(0, 10);
