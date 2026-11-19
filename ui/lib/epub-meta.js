// Metadata values from foliate-js are a string, a language map ({ en: "..." }),
// an object with a `name`, or an array of those. First usable string, trimmed, else null.
export function pickString(value) {
  if (typeof value === "string") return value.trim() || null;
  if (Array.isArray(value)) {
    for (const item of value) {
      const s = pickString(item);
      if (s) return s;
    }
    return null;
  }
  if (value && typeof value === "object") {
    return "name" in value ? pickString(value.name) : pickString(Object.values(value));
  }
  return null;
}

// The two font obfuscation methods (IDPF and Adobe) do not lock the text.
const FONT_METHODS = new Set(["http://www.idpf.org/2008/embedding", "http://ns.adobe.com/pdf/enc#RC"]);

// Copy protected: a rights.xml exists, or encryption.xml names any other method.
// A regex over the attributes is enough here; node tests have no DOMParser.
export function isProtected(rightsXml, encryptionXml) {
  if (typeof rightsXml === "string") return true;
  if (typeof encryptionXml !== "string") return false;
  for (const [, attrs] of encryptionXml.matchAll(/<(?:[\w.-]+:)?EncryptionMethod\b([^>]*)>/g)) {
    const algorithm = /\bAlgorithm\s*=\s*(["'])(.*?)\1/.exec(attrs)?.[2];
    if (!FONT_METHODS.has(algorithm)) return true;
  }
  return false;
}
