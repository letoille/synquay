/** Generate endpoints from the site opened in the browser, including its protocol and port. */
export function generatedOrigin(origin: string = window.location.origin): string {
  return new URL(origin).origin;
}
