import rss from "@astrojs/rss";
import type { APIContext } from "astro";
import { postsNewestFirst } from "../../lib/posts";

export async function GET(context: APIContext) {
  const posts = await postsNewestFirst();
  const blog = new URL("/blog/", context.site);
  const feed = new URL("rss.xml", blog);
  return rss({
    title: "Opal blog",
    description: "Release notes and writing about Opal, a JavaScript toolkit built on one module graph.",
    site: blog,
    xmlns: { atom: "http://www.w3.org/2005/Atom" },
    customData: [
      "<language>en</language>",
      `<lastBuildDate>${posts[0].data.date.toUTCString()}</lastBuildDate>`,
      `<atom:link href="${feed}" rel="self" type="application/rss+xml"/>`,
    ].join(""),
    items: posts.map((post) => ({
      title: post.data.title,
      link: new URL(`${post.id}/`, blog).href,
      pubDate: post.data.date,
      description: post.data.description,
    })),
  });
}
