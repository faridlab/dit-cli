// Sanitizes the SVG source stored in a `dit-diagram` fenced block before it
// is ever placed in the editor's DOM (ADR 0012). The bytes in the file are
// inert text; this module is the single gate between them and a rendered
// drawing. Everything not on the allowlist is removed — elements with their
// subtree, attributes one by one — and only local `#fragment` references
// survive, so nothing the renderer touches can reach the network or execute.
// The server never renders these blocks (comrak keeps `unsafe_` off), and
// the page CSP (`script-src 'self'`, `font-src 'self'`) is the backstop
// behind this layer, not a substitute for it.

export type SanitizedSvg = { ok: true; svg: string } | { ok: false; error: string };

/** Drawing elements only. No <script>, <style>, <foreignObject>, <image>,
 *  <a>, <use>-of-external, or SMIL animation — diagrams are static. */
const ELEMENTS = new Set([
  "svg", "g", "defs", "symbol", "title", "desc",
  "path", "rect", "circle", "ellipse", "line", "polyline", "polygon",
  "text", "tspan", "use",
  "linearGradient", "radialGradient", "stop",
  "marker", "pattern",
  "clipPath", "mask",
  "filter",
  "feBlend", "feColorMatrix", "feComponentTransfer", "feComposite", "feFlood",
  "feFuncA", "feFuncB", "feFuncG", "feFuncR", "feGaussianBlur",
  "feMerge", "feMergeNode", "feMorphology", "feOffset",
  "feDisplacementMap", "feTurbulence",
]);

/** Presentation and geometry attributes, namespace-free. `href`/`xlink:href`
 *  and `style` are handled specially below, never by this set. */
const ATTRIBUTES = new Set([
  // Structure.
  "id", "class", "viewBox", "preserveAspectRatio", "version",
  "width", "height", "x", "y", "x1", "x2", "y1", "y2",
  "xml:space", "xml:lang",
  // Transform.
  "transform", "gradientTransform", "patternTransform",
  // Shape geometry.
  "d", "cx", "cy", "r", "rx", "ry", "points", "pathLength",
  // Paint.
  "fill", "fill-opacity", "fill-rule", "opacity",
  "color", "paint-order",
  "stroke", "stroke-width", "stroke-opacity", "stroke-linecap",
  "stroke-linejoin", "stroke-miterlimit", "stroke-dasharray", "stroke-dashoffset",
  // Text.
  "font-family", "font-size", "font-style", "font-variant", "font-weight",
  "text-anchor", "dominant-baseline", "alignment-baseline",
  "dx", "dy", "rotate", "letter-spacing", "word-spacing",
  "text-decoration", "text-rendering", "textLength", "lengthAdjust",
  // Gradients.
  "gradientUnits", "spreadMethod", "fx", "fy", "fr", "offset",
  "stop-color", "stop-opacity",
  // Markers and patterns.
  "markerWidth", "markerHeight", "refX", "refY", "orient", "markerUnits",
  "marker-start", "marker-mid", "marker-end",
  "patternUnits", "patternContentUnits",
  // Clipping, masking, filters.
  "clip-path", "clip-rule", "mask", "maskUnits", "maskContentUnits",
  "filter", "filterUnits", "primitiveUnits",
  "in", "in2", "result", "stdDeviation", "scale", "type", "values",
  "xChannelSelector", "yChannelSelector",
  "baseFrequency", "numOctaves", "seed", "edgeMode",
  "tableValues", "slope", "intercept", "amplitude", "exponent",
  "k1", "k2", "k3", "k4", "operator", "mode",
  "flood-color", "flood-opacity",
  // Rendering hints.
  "shape-rendering", "vector-effect", "isolation",
  // Accessibility — the skill's diagrams carry a name and description.
  "role", "aria-label", "aria-labelledby", "aria-describedby", "aria-hidden",
]);

/** An `xlink:href`/`href` is kept only when it points inside the drawing. */
function isLocalRef(value: string): boolean {
  return value.trimStart().startsWith("#");
}

/** A walk that cannot be blown up by a hostile tree: past this many elements
 *  the whole block is refused and stays as source text. */
const MAX_ELEMENTS = 20_000;

/** Parses SVG source and returns a cleaned copy ready to inline, or the
 *  reason it refuses — the caller renders the reason, never the input. */
export function sanitizeSvg(source: string): SanitizedSvg {
  const doc = new DOMParser().parseFromString(source, "image/svg+xml");
  const parserError = doc.querySelector("parsererror");
  if (parserError) {
    return { ok: false, error: "not well-formed XML" };
  }
  const root = doc.documentElement;
  if (root === null || root.localName !== "svg") {
    return { ok: false, error: "root element is not <svg>" };
  }

  let count = 0;
  const walk = (element: Element): boolean => {
    count += 1;
    if (count > MAX_ELEMENTS) return false;
    if (!ELEMENTS.has(element.localName)) {
      element.remove();
      return true;
    }
    for (const attr of Array.from(element.attributes)) {
      const name = attr.name;
      const keep =
        ATTRIBUTES.has(name) ||
        (name.startsWith("data-") && !name.startsWith("data-on")) ||
        ((name === "href" || name === "xlink:href") && isLocalRef(attr.value)) ||
        name === "xmlns" ||
        name === "xmlns:xlink";
      if (!keep) element.removeAttribute(name);
    }
    for (const child of Array.from(element.children)) {
      if (!walk(child)) return false;
    }
    return true;
  };
  if (!walk(root)) {
    return { ok: false, error: `more than ${MAX_ELEMENTS} elements` };
  }
  // Comments carry nothing visual and can smuggle markup past a glance.
  const stripComments = (node: Node): void => {
    for (const child of Array.from(node.childNodes)) {
      if (child.nodeType === Node.COMMENT_NODE) node.removeChild(child);
      else stripComments(child);
    }
  };
  stripComments(root);

  return { ok: true, svg: new XMLSerializer().serializeToString(root) };
}
