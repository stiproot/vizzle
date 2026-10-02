import { INDEXABLE } from "~/config/site";

export function GET() {
  if (!INDEXABLE) {
    return new Response(
      `User-agent: *
Disallow: /
`,
      {
        headers: { "Content-Type": "text/plain" },
      },
    );
  }

  return new Response(
    `User-agent: *
Allow: /
`,
    {
      headers: { "Content-Type": "text/plain" },
    },
  );
}
