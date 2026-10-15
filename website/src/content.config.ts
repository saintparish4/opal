import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { z } from "astro/zod";

const posts = defineCollection({
  // The default id slugifies the file name, which would turn opal-v0.4.0 into
  // opal-v040 and break every published URL.
  loader: glob({
    pattern: "*.md",
    base: "./src/content/posts",
    generateId: ({ entry }) => entry.replace(/\.md$/, ""),
  }),
  schema: z.object({
    title: z.string(),
    description: z.string(),
    date: z.coerce.date(),
    release: z.string().url(),
    lede: z.string(),
    summary: z.string(),
    commands: z.array(z.object({ title: z.string(), id: z.string(), command: z.string() })),
  }),
});

export const collections = { posts };
