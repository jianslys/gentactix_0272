// GenTactix V2 frontend · GET /api/terrain.bin 二进制地形流解析器（黑盒）
// 格式：24B 头（"GTXV2TER" + u32 version + u32 rows + u32 cols + f32 cell_m）+ elev/kind/cover/smoke

import type { ParsedTerrainBinary, TerrainModel } from './types';

export function parseTerrainBinary(buf: ArrayBuffer): ParsedTerrainBinary {
  if (buf.byteLength < 24) {
    throw new Error(`Terrain binary too short: ${buf.byteLength} bytes`);
  }
  const view = new DataView(buf);
  const magic = String.fromCharCode(...new Uint8Array(buf, 0, 8));
  if (magic !== 'GTXV2TER') {
    throw new Error(`Invalid terrain magic: ${magic}`);
  }
  const version = view.getUint32(8, true);
  const rows = view.getUint32(12, true);
  const cols = view.getUint32(16, true);
  const cellM = view.getFloat32(20, true);
  const n = rows * cols;
  const expected = 24 + n * 7;
  if (buf.byteLength !== expected) {
    throw new Error(`Terrain binary length mismatch: got ${buf.byteLength}, expected ${expected}`);
  }

  const elevationM = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    elevationM[i] = view.getFloat32(24 + i * 4, true);
  }

  let off = 24 + n * 4;
  const kind = new Uint8Array(buf.slice(off, off + n));
  off += n;
  const coverRaw = new Uint8Array(buf, off, n);
  off += n;
  const smokeRaw = new Uint8Array(buf, off, n);

  const coverBonus = new Float32Array(n);
  const smokeDensity = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    coverBonus[i] = coverRaw[i]! / 255;
    smokeDensity[i] = smokeRaw[i]! / 255;
  }

  return { version, rows, cols, cellM, elevationM, kind, coverBonus, smokeDensity };
}

export function terrainBinaryToModel(parsed: ParsedTerrainBinary): TerrainModel {
  return {
    widthM: parsed.cols * parsed.cellM,
    heightM: parsed.rows * parsed.cellM,
    hills: [],
    patches: [],
    river: [],
    roads: [],
    binary: parsed,
  };
}
