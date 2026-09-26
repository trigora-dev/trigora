export function decodeTagged(value: unknown, heap?: unknown[], seen = new Set<number>()): unknown {
  if (!value || typeof value !== 'object' || !('t' in value)) {
    return value;
  }

  const tagged = value as { t: string; v?: unknown };

  switch (tagged.t) {
    case 'undefined':
      return null;
    case 'null':
      return null;
    case 'bool':
    case 'number':
    case 'string':
      return tagged.v;
    case 'ref': {
      const index = typeof tagged.v === 'number' ? tagged.v : -1;
      if (!heap || index < 0 || index >= heap.length || seen.has(index)) {
        return null;
      }
      seen.add(index);
      return decodeTagged(heap[index], heap, seen);
    }
    case 'cell':
      return decodeTagged(tagged.v, heap, seen);
    case 'array':
      return Array.isArray(tagged.v)
        ? tagged.v.map((entry) => decodeTagged(entry, heap, seen))
        : [];
    case 'object': {
      if (!tagged.v || typeof tagged.v !== 'object') {
        return {};
      }
      const result: Record<string, unknown> = {};
      for (const [key, entry] of Object.entries(tagged.v as Record<string, unknown>)) {
        result[key] = decodeTagged(entry, heap, seen);
      }
      return result;
    }
    default:
      return value;
  }
}

export function toJsonValue(value: unknown): import('@trigora/contracts').JsonValue {
  if (value === undefined) {
    return null;
  }

  return JSON.parse(JSON.stringify(value)) as import('@trigora/contracts').JsonValue;
}
