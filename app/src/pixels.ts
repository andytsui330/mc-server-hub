// Original 1.2.0 deterministic pixel art algorithms and palette.
const PX = (() => {
function rng(seed:number){ return () => { seed |= 0; seed = seed + 0x6D2B79F5 | 0; let t = Math.imul(seed ^ seed >>> 15, 1 | seed); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; }; }
type Grid = (string|null)[][];
const blank = ():Grid => Array.from({length: 16}, () => Array(16).fill(null));
function noise(sh:readonly string[], seed:number, w = [.55, .28, .17]){ const r = rng(seed), g = blank(); for (let y = 0; y < 16; y++) for (let x = 0; x < 16; x++) { const v = r(); g[y][x] = v < w[0] ? sh[0] : v < w[0] + w[1] ? sh[1] : sh[2]; } return g; }
const STONE = ["#8b8b8b", "#727272", "#9d9d9d"];
const ORE_SPOTS = [[3,3],[4,3],[3,4],[10,2],[11,3],[10,3],[6,8],[7,8],[7,9],[12,10],[12,11],[13,11],[3,12],[4,12],[4,13],[9,13]];
function ore(c1:string, c2:string, seed:number){ const g = noise(STONE, seed); ORE_SPOTS.forEach(([x, y], i) => g[y][x] = i % 3 === 0 ? c2 : c1); return g; }
function fromPx(pal:Record<string,string>, rows:readonly string[], id?:string){
  const w = Math.max(...rows.map(r => r.length)), h = rows.length, g = blank();
  if (rows.some(r => r.length !== w)) console.warn("圖示列寬不一致：", id);
  const ox = Math.floor((16 - w) / 2), oy = Math.floor((16 - h) / 2);
  rows.forEach((r, y) => [...r].forEach((c, x) => { if (c !== ".") g[y + oy][x + ox] = pal[c] || null; }));
  return g;
}
function grass(){ const g = noise(["#8a5a35", "#6b4226", "#9c6a40"], 11); const gr = noise(["#5fb84a", "#3f8f32", "#74c95c"], 12);
  for (let x = 0; x < 16; x++) { const depth = 3 + ((x * 7) % 3 === 0 ? 1 : 0); for (let y = 0; y < depth; y++) g[y][x] = gr[y][x]; } return g; }
function planks(){ const g = noise(["#b08a50", "#a07c45", "#bc965c"], 21, [.6, .25, .15]);
  for (let y = 0; y < 16; y++) for (let x = 0; x < 16; x++) { if (y % 4 === 3) g[y][x] = "#7a5a30"; if ((y >> 2) % 2 === 0 ? x === 5 : x === 12) if (y % 4 !== 3) g[y][x] = "#8a6a3a"; } return g; }
function log(){ const g = blank(), sh = ["#6b4a28", "#4f3519", "#7f5a32"], r = rng(31); for (let x = 0; x < 16; x++) { const base = x % 4 === 0 ? 1 : x % 4 === 2 ? 2 : 0; for (let y = 0; y < 16; y++) g[y][x] = r() < .15 ? sh[(base + 1) % 3] : sh[base]; } return g; }
function bricks(){ const g = blank(), r = rng(41); for (let y = 0; y < 16; y++) for (let x = 0; x < 16; x++) { const row = y >> 2, off = row % 2 ? 4 : 0;
  g[y][x] = (y % 4 === 3 || (x + off) % 8 === 7) ? "#cfc6b8" : (r() < .25 ? "#8e4030" : "#a5503c"); } return g; }
function glass(){ const g = blank(); for (let y = 0; y < 16; y++) for (let x = 0; x < 16; x++) { if (x === 0 || y === 0 || x === 15 || y === 15) g[y][x] = "#cfe8f5"; else g[y][x] = "#cfe8f51f"; }
  [[3,4],[4,3],[5,2],[3,6],[4,5],[5,4],[6,3],[11,10],[12,9],[10,11]].forEach(([x, y]) => g[y][x] = "#ffffffcc"); return g; }
function water(){ const g = noise(["#3f76e4", "#3566cc", "#5a8cf0"], 51); for (let x = 0; x < 16; x++) { g[(x % 8 < 4 ? 3 : 4)][x] = "#8fb4ff"; g[(x % 8 < 4 ? 10 : 11)][(x + 4) % 16] = "#8fb4ff"; } return g; }
function ice(){ const g = noise(["#9fd2f5", "#8cc4ee", "#b6defa"], 61); for (let i = 0; i < 6; i++) { g[2 + i][9 - i] = "#ffffffbb"; g[8 + i][13 - i] = "#ffffffbb"; } return g; }
function pumpkin(){ const g = blank(), r = rng(71); for (let y = 0; y < 16; y++) for (let x = 0; x < 16; x++) g[y][x] = x % 5 === 0 ? "#c96a12" : r() < .2 ? "#e07e1a" : "#f0901f"; for (let x = 6; x < 10; x++) g[0][x] = "#5a8a2a"; return g; }
function leaves(){ const g = noise(["#3f8f32", "#2e6e24", "#5fb84a"], 81, [.45, .3, .25]); const r = rng(82); for (let y = 0; y < 16; y++) for (let x = 0; x < 16; x++) if (r() < .06) g[y][x] = "#1f4f18"; return g; }

/* 生物臉：8×8 畫、放大 2 倍剛好 16×16 */
function face(pal:Record<string,string>, rows:readonly string[], id?:string){ if (rows.length !== 8 || rows.some(r => r.length !== 8)) console.warn("生物臉不是 8×8：", id);
  const g = blank(); rows.forEach((r, y) => [...r].forEach((c, x) => { if (c !== ".") { const v = pal[c] || null; g[y*2][x*2] = g[y*2][x*2+1] = g[y*2+1][x*2] = g[y*2+1][x*2+1] = v; } })); return g; }
const MOBS: [string,string,string,Record<string,string>,string[]][] = [
  ["友善生物", "pig", "豬", {p: "#f2a5b0", P: "#d97a8a", k: "#222", w: "#fff", n: "#b85a6a"}, ["pppppppp", "pppppppp", "pwkppkwp", "pppppppp", "ppPPPPpp", "ppPnnPpp", "ppPPPPpp", "pppppppp"]],
  ["友善生物", "cow", "牛", {h: "#d8d8c0", b: "#4a3424", w: "#f4f4f4", k: "#111", n: "#d8b0b0", N: "#8a5a5a"}, ["h......h", "bbbwwbbb", "bkbwwbkb", "bbbwwbbb", "bbwwwwbb", "bnnnnnnb", "bnNnnNnb", "bnnnnnnb"]],
  ["友善生物", "sheep", "羊", {w: "#f4f4f4", W: "#dcdcdc", f: "#e8c8a8", k: "#222", p: "#f0a0a8"}, ["wWwwwwWw", "wwffffww", "wffffffw", "wfkffkfw", "wffffffw", "wffppffw", "wwffffww", "wWwwwwWw"]],
  ["友善生物", "chicken", "雞", {w: "#f4f4f4", k: "#222", y: "#ffb02a", r: "#e8221c"}, ["..wwww..", ".wwwwww.", ".wkwwkw.", ".wwwwww.", ".yyyyyy.", ".wwrrww.", ".wwrrww.", ".wwwwww."]],
  ["友善生物", "horse", "馬", {b: "#8a5a35", B: "#6b4226", m: "#3a2416", k: "#111", n: "#3a2416"}, ["Bb.mm.bB", "bbbmmbbb", "bkbmmbkb", "bbbbbbbb", ".bbBBbb.", ".bBbbBb.", ".bnbbnb.", ".bbbbbb."]],
  ["友善生物", "rabbit", "兔子", {b: "#c8a888", w: "#f4f4f4", k: "#222", p: "#f0a0a8"}, [".bb..bb.", ".bb..bb.", "bbbbbbbb", "bkbbbbkb", "bbbppbbb", "bbwwwwbb", ".bwwwwb.", "..bbbb.."]],
  ["友善生物", "wolf", "狼", {w: "#e0e0e0", W: "#b8b8b8", k: "#222"}, ["ww....ww", "wWwwwwWw", "wwwwwwww", "wkwwwwkw", "wwwWWwww", "wwWkkWww", ".wWWWWw.", "..wwww.."]],
  ["友善生物", "cat", "貓", {o: "#e8a040", O: "#c07a20", g: "#7fd04a", k: "#222", p: "#ff9db0"}, ["o......o", "oo....oo", "oOooooOo", "ogkoogko", "oooppooo", "ooOkkOoo", "oooooooo", ".oooooo."]],
  ["友善生物", "fox", "狐狸", {o: "#e8782a", w: "#f4f4f4", k: "#222"}, ["o......o", "oo....oo", "oooooooo", "okooooko", "wwooooww", "wwwkkwww", ".wwwwww.", "..wwww.."]],
  ["友善生物", "panda", "熊貓", {w: "#f4f4f4", k: "#222", K: "#3a3a3a"}, ["kk....kk", "kwwwwwwk", "wwwwwwww", "wKkwwkKw", "wKKwwKKw", "wwwkkwww", "wwwwwwww", ".wwwwww."]],
  ["友善生物", "bee", "蜜蜂", {y: "#f5c542", k: "#222", w: "#fff", b: "#3a2a10"}, ["yyyyyyyy", "yyyyyyyy", "ykwyykwy", "ykkyykky", "yyyyyyyy", "bbbbbbbb", "yyyyyyyy", "bbbbbbbb"]],
  ["友善生物", "parrot", "鸚鵡", {r: "#e8221c", R: "#b81810", y: "#ffd84a", k: "#222"}, ["..rrrr..", ".rrrrrr.", "rrrrrrrr", "rkrrrrkr", "rrryyrrr", "rrryyrrr", ".rRRRRr.", "..RRRR.."]],
  ["友善生物", "villager", "村民", {s: "#c69c7c", S: "#a87c5c", b: "#4a2e1a", e: "#3a7a2a", w: "#fff"}, ["ssssssss", "ssssssss", "sbbbbbbs", "sweSSews", "sssSSsss", "sssSSsss", "sssSSsss", "ssbbbbss"]],
  ["友善生物", "golem", "鐵巨人", {i: "#d8d0c4", I: "#b0a89c", r: "#e8221c", k: "#555", v: "#4f8f3a"}, ["iiiiiiii", "iIiiiiIi", "iIIIIIIi", "irkiikri", "iiiIIiii", "iviIIivi", "iiiIIiii", "iiIIIIii"]],
  ["友善生物", "axolotl", "美西螈", {p: "#ffb0c8", P: "#ff80a8", g: "#e05080", k: "#222"}, ["g.pppp.g", "gppppppg", "gpkppkpg", ".pppppp.", "gppPPppg", ".pppppp.", "..pppp..", "........"]],
  ["友善生物", "frog", "青蛙", {o: "#d0884a", O: "#a8682a", w: "#fff", k: "#222", g: "#e8b070"}, [".ww..ww.", "wkwoowkw", "oooooooo", "oOooooOo", "kkkkkkkk", "oggggggo", ".oooooo.", "........"]],
  ["友善生物", "turtle", "烏龜", {g: "#5fb84a", G: "#3f8f32", s: "#6f8a3a", S: "#4a6a20"}, ["..gGGg..", ".GssssG.", "gsSssSsg", "GssSSssG", "GssSSssG", "gsSssSsg", ".GssssG.", "..gGGg.."]],
  ["友善生物", "squid", "魷魚", {q: "#2a4f7a", Q: "#1e3a5a", w: "#fff", k: "#111"}, ["qqqqqqqq", "qQqqqqQq", "qkwqqwkq", "qqqqqqqq", "qQqqqqQq", "qqQqqQqq", "q.qq.qq.", "q..q..q."]],
  ["友善生物", "dolphin", "海豚", {b: "#7aa8c8", B: "#5a88a8", w: "#dfe8ef", k: "#111"}, ["...bb...", "..bbbb..", ".bbbbbb.", "bkbbbbkb", "bbbbbbbb", "bwwwwwwb", ".wwBBww.", "..wwww.."]],
  ["敵對生物", "creeper", "苦力怕", {g: "#5fb84a", G: "#3f8f32", k: "#111"}, ["gGggGggG", "ggGgggGg", "gkkggkkg", "gkkggkkg", "gGgkkgGg", "ggkkkkgg", "gGkkkkGg", "ggkGGkgg"]],
  ["敵對生物", "zombie", "殭屍", {z: "#4f8f3a", Z: "#3a6e2a", h: "#2e5a20", k: "#111"}, ["hhhhhhhh", "hzhhhhzh", "zzzzzzzz", "zkkzzkkz", "zzzZZzzz", "zzZzzZzz", "zzZZZZzz", "zzzzzzzz"]],
  ["敵對生物", "skeleton", "骷髏", {w: "#d8d8d8", W: "#b0b0b0", k: "#222"}, ["wwwwwwww", "wWwwwwWw", "wwwwwwww", "wkkwwkkw", "wwwkkwww", "wWwwwwWw", "wkkkkkkw", "wwwwwwww"]],
  ["敵對生物", "spider", "蜘蛛", {s: "#3a3030", S: "#2a2222", r: "#e8221c"}, ["ssSssSss", "sSssssSs", "ssssssss", "srsrrsrs", "srrssrrs", "ssSssSss", "sSsSSsSs", "ssssssss"]],
  ["敵對生物", "enderman", "終界使者", {k: "#111111", K: "#1e1e24", p: "#c86bff", P: "#e6b3ff"}, ["kkkkkkkk", "kKkkkKkk", "kkkkkkkk", "kkkkkkkk", "pPpkkpPp", "kkkkkkkk", "kKkkkkKk", "kkkkkkkk"]],
  ["敵對生物", "slime", "史萊姆", {l: "#7ed957", L: "#5fb84a", d: "#2e6e24", k: "#1f3a1a"}, ["llllllll", "lLLLLLLl", "lLddLddl", "lLkdLkdl", "lLLLLLLl", "lLLLdLLl", "lLLLLLLl", "llllllll"]],
  ["敵對生物", "witch", "女巫", {h: "#3a1a4a", g: "#4fae3a", s: "#c69c7c", S: "#a87c5c", w: "#fff", e: "#6a2a8a", b: "#4a2e1a"}, ["...hh...", "..hhhh..", ".hggggh.", "hhhhhhhh", "swessews", "sssSSsss", "sssSSsss", "ssbSSbss"]],
  ["敵對生物", "pillager", "掠奪者", {s: "#8a8f8f", S: "#6a6f6f", b: "#2a2a2a", k: "#222", w: "#fff"}, ["ssssssss", "ssssssss", "bbbbbbbb", "swksskws", "sssSSsss", "sssSSsss", "ssbbbbss", "sbssssbs"]],
  ["敵對生物", "blaze", "烈焰人", {y: "#ffd84a", o: "#ffae2a", k: "#4a2a00"}, ["yyyyyyyy", "yoyyyyoy", "yyyyyyyy", "ykkyykky", "yyyooyyy", "yoyyyyoy", "yyokkoyy", "yyyyyyyy"]],
  ["敵對生物", "ghast", "地獄幽靈", {w: "#f4f4f4", W: "#d8d8d8", k: "#333"}, ["wwwwwwww", "wWwwwwWw", "wwwwwwww", "wkkwwkkw", "wWwwwwWw", "wwkkkkww", "wwkkkkww", "wwwwwwww"]],
  ["敵對生物", "piglin", "豬布林", {p: "#e8a0a0", P: "#c88080", k: "#222", w: "#fff", n: "#8a4040", g: "#ffd84a"}, ["pppppppp", "pPppppPp", "pwkppkwp", "pppppppp", "ppPPPPpp", "ppPnnPpp", "gpPPPPpp", "pppppppp"]],
  ["敵對生物", "wither", "凋零", {k: "#2a2a2a", K: "#1a1a1a", w: "#e8e8e8"}, ["kkkkkkkk", "kKkkkkKk", "kwwkkwwk", "kkkkkkkk", "kkKkkKkk", "kwkwwkwk", "kkwkkwkk", "kkkkkkkk"]],
  ["敵對生物", "dragon", "終界龍", {k: "#1a1a1a", K: "#2e2e2e", p: "#c86bff"}, ["k......k", "kK....Kk", "kkkkkkkk", "kppkkppk", "kkkkkkkk", "KkkkkkkK", "kkKkkKkk", "kkkkkkkk"]],
  ["敵對生物", "warden", "伏守者", {t: "#0f3a3a", T: "#0a2a2a", c: "#3ae0d0", k: "#061818"}, ["TtTttTtT", "tttttttt", "tTttttTt", "tttttttt", "tcttttct", "tttttttt", "tkkkkkkt", "tttttttt"]],
];
const GEM = ["....kkkkkkkk....", "...kCwCccCCck...", "..kCCCcccccCck..", ".kccccdcccdccck.", "..kcccdcccdcck..", "...kccdcccdck...", "....kcdcccdk....", ".....kdccdk.....", "......kcck......", ".......kk......."];
const INGOT = ["..kkkkkkkkkk..", ".kwwwwyyyyyyk.", "kwyyyyyyyyyyYk", "kyyyyyyyyyyYYk", "kYYYYYYYYYYYYk", ".kkkkkkkkkkkk."];
const K = "#1b1b1b";
const ICONS = [
  // 方塊
  {cat: "方塊", id: "grass", n: "草地", g: grass},
  {cat: "方塊", id: "dirt", n: "泥土", g: () => noise(["#8a5a35", "#6b4226", "#9c6a40"], 2)},
  {cat: "方塊", id: "stone", n: "石頭", g: () => noise(STONE, 3)},
  {cat: "方塊", id: "cobble", n: "圓石", g: () => noise(["#7a7a7a", "#5c5c5c", "#959595"], 4, [.4, .35, .25])},
  {cat: "方塊", id: "sand", n: "沙子", g: () => noise(["#e3d39a", "#d4c182", "#efe2b0"], 5)},
  {cat: "方塊", id: "snow", n: "雪", g: () => noise(["#f4f8fb", "#e2eaf0", "#ffffff"], 6)},
  {cat: "方塊", id: "ice", n: "冰", g: ice},
  {cat: "方塊", id: "water", n: "水", g: water},
  {cat: "方塊", id: "lava", n: "岩漿", g: () => noise(["#ff8a1f", "#e8590c", "#ffc23a"], 8, [.45, .3, .25])},
  {cat: "方塊", id: "planks", n: "木板", g: planks},
  {cat: "方塊", id: "log", n: "原木", g: log},
  {cat: "方塊", id: "leaves", n: "樹葉", g: leaves},
  {cat: "方塊", id: "bricks", n: "磚塊", g: bricks},
  {cat: "方塊", id: "glass", n: "玻璃", g: glass},
  {cat: "方塊", id: "pumpkin", n: "南瓜", g: pumpkin},
  {cat: "方塊", id: "obsidian", n: "黑曜石", g: () => noise(["#1e1430", "#140d22", "#3a2a5a"], 9, [.5, .3, .2])},
  {cat: "方塊", id: "nether", n: "地獄岩", g: () => noise(["#7a2a2a", "#5e1e1e", "#9a3a36"], 10)},
  {cat: "方塊", id: "endstone", n: "終界石", g: () => noise(["#e6e3a6", "#d2cf8e", "#f2efbe"], 13)},
  {cat: "方塊", id: "amethyst", n: "紫水晶", g: () => noise(["#9a62d8", "#7a44b8", "#c79bff"], 14, [.45, .3, .25])},
  // 礦石
  {cat: "礦石", id: "ore-diamond", n: "鑽石礦", g: () => ore("#4fe3d6", "#a8fff6", 15)},
  {cat: "礦石", id: "ore-gold", n: "金礦", g: () => ore("#ffd84a", "#fff1a0", 16)},
  {cat: "礦石", id: "ore-emerald", n: "綠寶石礦", g: () => ore("#2fd16a", "#a0ffc0", 17)},
  {cat: "礦石", id: "ore-redstone", n: "紅石礦", g: () => ore("#e8221c", "#ff6a5a", 18)},
  {cat: "礦石", id: "ore-lapis", n: "青金石礦", g: () => ore("#2a4fc8", "#5a80ff", 19)},
  {cat: "礦石", id: "ore-iron", n: "鐵礦", g: () => ore("#d8b49a", "#f0d6c2", 20)},
  {cat: "礦石", id: "ore-coal", n: "煤礦", g: () => ore("#2a2a2a", "#474747", 22)},
  // 寶物
  {cat: "寶物", id: "diamond", n: "鑽石", g: () => fromPx({k: "#0d3b3a", c: "#4fe3d6", C: "#a8fff6", d: "#1fa79c", w: "#fff"}, GEM)},
  {cat: "寶物", id: "emerald", n: "綠寶石", g: () => fromPx({k: "#0b3a1c", c: "#2fd16a", C: "#a0ffc0", d: "#14a04a", w: "#fff"}, GEM)},
  {cat: "寶物", id: "ruby", n: "紅寶石", g: () => fromPx({k: "#3a0b10", c: "#e8323c", C: "#ff9aa0", d: "#a8182a", w: "#fff"}, GEM)},
  {cat: "寶物", id: "gold", n: "金錠", g: () => fromPx({k: "#5a3a00", y: "#ffd84a", Y: "#e0a820", w: "#fff6c2"}, INGOT)},
  {cat: "寶物", id: "iron", n: "鐵錠", g: () => fromPx({k: "#2a2a2a", y: "#d4d8dc", Y: "#a2a8ae", w: "#ffffff"}, INGOT)},
  {cat: "寶物", id: "crown", n: "王冠", g: () => fromPx({y: "#ffd84a", Y: "#e0a820", r: "#e8413c", b: "#4fa3e3"}, ["y....yy....y", "yy..yyyy..yy", "yyyyyyyyyyyy", "yyrYYbbYYryy", "yyyyyyyyyyyy", "YYYYYYYYYYYY"])},
  {cat: "寶物", id: "star", n: "星星", g: () => fromPx({y: "#ffd84a", o: "#e0a820"}, [".....y.....", "....yyy....", "....yyy....", "yyyyyyyyyyy", ".yyyyyyyyy.", "..yyyyyyy..", "..yyyoyyy..", ".yyyo.oyyy.", ".yyo...oyy.", "yo.......oy"])},
  {cat: "寶物", id: "heart", n: "愛心", g: () => fromPx({k: "#2a0a0a", r: "#e83c4a", w: "#ffb3b8", R: "#a8202d"}, [".kkk...kkk.", "krrrk.krrrk", "krwrrkrrrRk", "krrrrrrrrRk", ".krrrrrrRk.", "..krrrrRk..", "...krrRk...", "....kRk....", ".....k....."])},
  // 工具與物品
  {cat: "工具與物品", id: "sword", n: "劍", g: () => fromPx({k: K, s: "#dfe6ea", S: "#9aa7ad", h: "#7a4a22", g: "#e2b23a"}, ["...........kkk", "..........kssk", ".........ksSk.", "........ksSk..", ".......ksSk...", "......ksSk....", ".kk..ksSk.....", ".kgkksSk......", "..kgsSk.......", "...kgk........", "..khkgk.......", ".khk.kgk......", "kkk...kk......"])},
  {cat: "工具與物品", id: "pickaxe", n: "鎬", g: () => fromPx({k: K, s: "#8fd6e0", S: "#4fb5c2", h: "#7a4a22", H: "#5a3416"}, ["...kkkkkkk....", "..ksssssssk...", ".ksSkkkkkSsk..", "ksSk..khk.kSsk", "kSk...khk..kSk", "kk....khk...kk", "......khk.....", "......khk.....", "......kHk.....", "......khk.....", "......kHk.....", "......kkk....."])},
  {cat: "工具與物品", id: "axe", n: "斧頭", g: () => fromPx({k: K, s: "#c9d1d6", S: "#8a969c", h: "#7a4a22", H: "#5a3416"}, ["..kkkkkk....", ".kssssSkk...", "kssssSkhk...", "kssssSkhk...", ".kssSkkhk...", "..kkk.khk...", "......khk...", "......kHk...", "......khk...", "......khk...", "......kHk...", "......kkk..."])},
  {cat: "工具與物品", id: "shield", n: "盾牌", g: () => fromPx({k: K, m: "#b8bec4", r: "#3a6fd8", w: "#f5c542"}, ["kkkkkkkkkkkk", "kmmmmmmmmmmk", "kmrrrwwrrrmk", "kmrrrwwrrrmk", "kmwwwwwwwwmk", "kmrrrwwrrrmk", "kmrrrwwrrrmk", ".kmrrwwrrmk.", "..kmrwwrmk..", "...kmmmmk...", "....kkkk...."])},
  {cat: "工具與物品", id: "bomb", n: "炸彈", g: () => fromPx({k: K, g: "#3a3d42", G: "#5a5f66", w: "#9aa0a6", f: "#ffae2a", y: "#ffd84a", b: "#7a4a22"}, ["........yf..", ".......fb...", "......kb....", "...kkkkk....", "..kggggggk..", ".kgwGgggggk.", "kgwggggggggk", "kgGggggggggk", "kggggggggggk", ".kggggggggk.", "..kggggggk..", "...kkkkkk..."])},
  {cat: "工具與物品", id: "torch", n: "火把", g: () => fromPx({y: "#ffd84a", o: "#ff8a1f", w: "#fff6c2", k: K, h: "#7a4a22"}, ["..yy..", ".yowy.", ".ooyo.", "..oo..", "..kk..", "..hh..", "..hh..", "..hh..", "..hh..", "..hh..", "..hh..", "..hh.."])},
  {cat: "工具與物品", id: "potion", n: "藥水", g: () => fromPx({k: K, g: "#cfe8f0", p: "#c64ad8", P: "#8a2aa0", w: "#fff", c: "#8a5a35"}, ["...kkkk...", "...kcck...", "...kggk...", "..kggggk..", ".kggggggk.", "kgwpppppgk", "kgppppppPk", "kgppppppPk", "kgpppppPPk", ".kgPPPPPk.", "..kkkkkk.."])},
  {cat: "工具與物品", id: "chest", n: "寶箱", g: () => fromPx({k: "#2a1a0a", w: "#b07a3a", W: "#8a5a28", d: "#6b4220", m: "#c9c9c9"}, ["kkkkkkkkkkkkkk", "kwwwwwwwwwwwwk", "kwWWWWWWWWWWwk", "kwWWWWWWWWWWwk", "kkkkkkmmkkkkkk", "kwwwwwmmwwwwwk", "kwWWWWkkWWWWwk", "kwWWWWWWWWWWwk", "kwWWWWWWWWWWwk", "kwddddddddddwk", "kkkkkkkkkkkkkk"])},
  {cat: "工具與物品", id: "book", n: "書", g: () => fromPx({k: K, r: "#8a2a2a", R: "#5a1a1a", w: "#f4ecd8"}, ["kkkkkkkkkkk.", "krrrrrrrrrk.", "krrRRRRRrrk.", "krrrrrrrrrk.", "krrRRRRrrrk.", "krrrrrrrrrk.", "krrrrrrrrrkk", "kkkkkkkkkkwk", ".kwwwwwwwwwk", "..kkkkkkkkkk"])},
  {cat: "工具與物品", id: "map", n: "地圖", g: () => fromPx({k: K, p: "#e8d9b0", P: "#c9b88a", g: "#6fbf5a", b: "#5aa9e6", r: "#e8413c"}, ["kkkkkkkkkkkk", "kpgggppbbbpk", "kpggpppbbbpk", "kpppprppbbpk", "kpbbppprppgk", "kpbbbpppppgk", "kppbppggpggk", "kPpppgggpppk", "kkkkkkkkkkkk"])},
  {cat: "工具與物品", id: "compass", n: "指南針", g: () => fromPx({k: K, g: "#9aa0a6", G: "#5f656b", w: "#e8e8e8", r: "#e8413c"}, ["...kkkkkk...", ".kkggggggkk.", ".kgwwwwwwgk.", "kgwwwwrwwwgk", "kgwwwwrwwwgk", "kgwwwrrwwwgk", "kgwwwkwwwwgk", "kgwwwkwwwwgk", ".kgwwwwwwgk.", ".kkGGGGGGkk.", "...kkkkkk..."])},
  {cat: "工具與物品", id: "pad", n: "手把", g: () => fromPx({k: K, g: "#5a5f66", w: "#e8e8e8", r: "#e8413c", b: "#4fa3e3"}, [".kkkkkkkkkkkk.", "kggggggggggggk", "kggwggggggrggk", "kgwwwgggggbgrk", "kggwggggggrggk", "kggggggggggggk", "kgggkkkkkkgggk", ".kkk......kkk."])},
  // 食物
  {cat: "食物", id: "apple", n: "蘋果", g: () => fromPx({k: "#2a0a0a", r: "#e8413c", R: "#b52a26", w: "#ff9d8f", g: "#4caf50", b: "#6b4226"}, ["......kk....", ".....kbkkk..", ".....kbkggk.", "..kkkkkkkk..", ".krrrrrrrrk.", "krwwrrrrrrrk", "krwrrrrrrrrk", "krrrrrrrrrRk", "krrrrrrrrrRk", "krrrrrrrrRRk", ".krrrrrrRRk.", "..kRRkkRRk..", "...kk..kk..."])},
  {cat: "食物", id: "carrot", n: "胡蘿蔔", g: () => fromPx({k: K, o: "#ff8a1f", O: "#d9650f", g: "#4caf50"}, ["......g.g.", ".....ggg..", "....kkgg..", "...koOk...", "..kooOk...", "..koOk....", ".kooOk....", ".kOok.....", "kOok......", "kk........"])},
  {cat: "食物", id: "cake", n: "蛋糕", g: () => fromPx({k: "#2a1a0a", w: "#fff6ee", W: "#e6dccf", r: "#e8413c", b: "#c58a4a", B: "#9a6232"}, ["..kkkkkkkkkk..", ".kwwrwwwwrwwk.", "kwwwwwwrwwwwwk", "kwWwwWwwwWwwWk", "kbWbbWbbbWbbbk", "kbbbbbbbbbbbbk", "kBbbbbbbbbbbBk", "kBBbbbbbbbbBBk", ".kkkkkkkkkkkk."])},
  {cat: "食物", id: "fish", n: "魚", g: () => fromPx({k: K, b: "#5aa9e6", B: "#2f6fa8", w: "#fff"}, ["....kkkkkk....", "..kkbbbbbbk.kk", ".kbbbbbbbbbkbk", "kbwkbbbbbbbbbk", "kbbbbbbbbbbbbk", ".kBBbbbbbbbkBk", "..kkBBBBBBk.kk", "....kkkkkk...."])},
  {cat: "食物", id: "mushroom", n: "蘑菇", g: () => fromPx({k: K, r: "#e8413c", w: "#fff", s: "#efe3cf"}, ["...kkkkkk...", ".krrwwrrrrk.", "krwwrrrrwwrk", "krwwrrrrwwrk", "krrrrwwrrrrk", "kkkkkkkkkkkk", "...kssssk...", "...kssssk...", "...kssssk...", "...kkkkkk..."])},
  ...MOBS.map(([cat, id, n, pal, rows]) => ({cat, id, n, g: () => face(pal, rows, id)})),
  // 自然
  {cat: "自然", id: "tree", n: "樹", g: () => fromPx({t: "#4caf50", T: "#2e7d32", b: "#7a4a22"}, ["...tttttt...", ".ttTttttTtt.", "tttttTtttttt", "tTttttttTttt", "ttttTttttttT", ".tttttTtttt.", "...ttbbtt...", ".....bb.....", ".....bb.....", ".....bb.....", ".....bb....."])},
  {cat: "自然", id: "flower", n: "花", g: () => fromPx({r: "#e8413c", y: "#ffd84a", g: "#4caf50"}, ["..rrrr..", ".rryyrr.", ".rryyrr.", "..rrrr..", "...gg...", ".g.gg...", ".gggg.g.", "...gggg.", "...gg...", "...gg..."])},
  {cat: "自然", id: "fire", n: "火焰", g: () => fromPx({r: "#e8413c", o: "#ff8a1f", y: "#ffd84a"}, ["....o.....", "...oo..o..", "..ooo.oo..", ".oooyooo.o", ".ooyyyoooo", "rooyyyyooo", "roooyyyoor", "rroooyoorr", ".rrroooorr", "..rrrrrrr."])},
  // 場景
  {cat: "場景", id: "island", n: "空島", g: () => fromPx({t: "#4caf50", b: "#7a4a22", g: "#5fb84a", G: "#3f8f32", d: "#8a5a35", D: "#6b4226"}, ["......ttt.......", ".....ttttt......", ".....ttttt......", "......tbt.......", ".......b........", "..gggggbgggggg..", ".gGggggggGgggGg.", ".dddddddddddddd.", "..dDddddDdddDd..", "...ddddddddd....", "....dDdddDd.....", ".....ddddd......", "......dDd.......", ".......d........"])},
  {cat: "場景", id: "castle", n: "城堡", g: () => fromPx({k: K, s: "#a9afb5", S: "#7d848a", d: "#5a3a20", f: "#e8413c"}, ["......kf......", "......kff.....", "......k.......", "kkk..kkkk..kkk", "ksk..kssk..ksk", "kskkkksskkkksk", "kssssssssssssk", "ksSsssssssSssk", "kssssskksssssk", "ksssskddkssssk", "ksSsskddksSssk", "kkkkkkkkkkkkkk"])},
  {cat: "場景", id: "house", n: "小屋", g: () => fromPx({k: K, r: "#c0392b", R: "#8e2a20", w: "#d9b27a", d: "#6b4220", g: "#9fd8ff"}, ["......kk......", ".....krrk.....", "....krrrrk....", "...krrRrrrk...", "..krrrrrRrrk..", ".krrRrrrrrrrk.", "kkkkkkkkkkkkkk", ".kwwwwwwwwwwk.", ".kwggwwwwddwk.", ".kwggwwwwddwk.", ".kwwwwwwwddwk.", ".kkkkkkkkkkkk."])},
  {cat: "場景", id: "portal", n: "傳送門", g: () => fromPx({o: "#2a1a3a", O: "#1a1024", p: "#a24ce0", P: "#d48bff"}, ["oooooooooooo", "oOooooooooOo", "oopPppppPpoo", "ooppPpppppoo", "oopppPPpppoo", "ooPpppppPpoo", "oopPppPpppoo", "oopppppPpPoo", "ooPppPppppoo", "oopppppPppoo", "oOooooooooOo", "oooooooooooo"])},
  {cat: "場景", id: "moon", n: "月亮", g: () => fromPx({m: "#f4f1d0", M: "#c9c497"}, ["...mmmm...", ".mmmmm....", "mmmmm.....", "mmmm......", "mmmm......", "mmmM......", "mmmmM.....", ".mmmmMMmm.", "...mmmMm.."])},
  {cat: "場景", id: "sun", n: "太陽", g: () => fromPx({y: "#ffd84a", o: "#ffae2a"}, [".....yy.....", ".y...yy...y.", "..y......y..", "....oooo....", "...oyyyyo...", "yy.oyyyyo.yy", "yy.oyyyyo.yy", "...oyyyyo...", "....oooo....", "..y......y..", ".y...yy...y.", ".....yy....."])},
];
const CACHE:Record<string,Grid> = {};
function draw(cv:HTMLCanvasElement, id:string){ const g = CACHE[id] || (CACHE[id] = (ICONS.find(i => i.id === id)??ICONS[0]).g()); const c = cv.getContext("2d"); if(!c)return; c.clearRect(0, 0, 16, 16);
  g.forEach((row, y) => row.forEach((col, x) => { if (col) { c.fillStyle = col; c.fillRect(x, y, 1, 1); } })); }
const COLORS = [["綠", "#5fd39a"], ["冰藍", "#57c1ff"], ["紫", "#b48cff"], ["粉", "#ff8ab8"], ["紅", "#ff6b6b"], ["橘", "#ffa45c"], ["黃", "#f2d16b"], ["白", "#f4f4f6"]];
return {ICONS, COLORS, draw};
})();


export const pixelArt = {icons:PX.ICONS.map(i=>({id:i.id,name:i.n,cat:i.cat,pixels:i.g()})),colors:PX.COLORS};
