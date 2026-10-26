import { mkdir, writeFile } from 'node:fs/promises';

// C / Flow: opposing ribbons create an S with a continuous negative-space channel.
// Keep the geometry here as the single source for all exported SVG variants.
const ribbon = 'M105 16H232L176 65Q168 72 157 72H110C91 72 80 82 80 96C80 103 84 110 91 115L154 158H81C47 158 24 139 24 111C24 90 35 72 51 57L91 22Q97 16 105 16Z';
const mark = `<path d="${ribbon}"/><path d="${ribbon}" transform="rotate(180 128 128)"/>`;
const svg = (body) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="none"><title>Synquay Flow</title>${body}</svg>\n`;
const tile = (background, foreground) => svg(`<rect width="256" height="256" rx="58" fill="${background}"/><g transform="translate(28 28) scale(.78125)" fill="${foreground}">${mark}</g>`);
const assets = {
  'flow-mark.svg': svg(`<g fill="#4263df">${mark}</g>`),
  'flow-mark-mono.svg': svg(`<g fill="currentColor">${mark}</g>`),
  'flow-icon.svg': tile('#4263df', '#ffffff'),
  'flow-icon-light.svg': tile('#eef2ff', '#4263df'),
  'flow-icon-dark.svg': tile('#141e30', '#ffffff'),
};
const destination = new URL('../frontend/public/brand/', import.meta.url);
await mkdir(destination, { recursive: true });
for (const [name, content] of Object.entries(assets)) await writeFile(new URL(name, destination), content);
console.log(`Generated ${Object.keys(assets).length} Synquay Flow assets.`);
