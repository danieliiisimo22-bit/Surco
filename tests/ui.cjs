const { test } = require("node:test");
const assert = require("node:assert/strict");
const vm = require("node:vm");
const fs = require("node:fs");
const path = require("node:path");
function context() {
  const nodes = new Map();
  const listeners = {};
  const document = {
    querySelector(s) {
      if (!nodes.has(s))
        nodes.set(s, {
          innerHTML: "",
          textContent: "",
          hidden: false,
          append() {},
          scrollIntoView() {},
          classList: { toggle() {} },
        });
      return nodes.get(s);
    },
    querySelectorAll() {
      return [];
    },
    addEventListener(n, fn) {
      listeners[n] = fn;
    },
    createElement() {
      return { remove() {} };
    },
  };
  const c = vm.createContext({
    document,
    console,
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
    AbortController,
    Set,
  });
  vm.runInContext(
    fs
      .readFileSync(path.join(__dirname, "../web/app.js"), "utf8")
      .replace(/init\(\);\s*$/, ""),
    c,
  );
  vm.runInContext(
    "state.config={directory:'C:/Music',format:'mp3',browser:''};",
    c,
  );
  c.listeners = listeners;
  return c;
}
test("old search cannot replace a newer album request", async () => {
  const c = context();
  await vm.runInContext(
    `(async()=>{let resolve;api=()=>new Promise(r=>resolve=r);const old=search('old');state.request++;state.page='jobs';state.busy=false;resolve([{id:'stale'}]);await old;})()`,
    c,
  );
  assert.equal(vm.runInContext("state.results.length", c), 0);
  assert.equal(vm.runInContext("state.busy", c), false);
});
test("leaving a loading album prevents late results from reopening it", async () => {
  const c = context();
  await vm.runInContext(
    `(async()=>{let resolve;api=()=>new Promise(r=>resolve=r);const old=openAlbum('a');state.request++;state.page='settings';resolve({id:'a',tracks:[]});await old;})()`,
    c,
  );
  assert.equal(vm.runInContext("state.album", c), null);
});
test("destination album is explicit for audio reused from another publication", () => {
  const c = context();
  const html = vm.runInContext(
    `albumView({title:'New album',artist:'Artist',year:'2000',tracks:[{number:1,title:'Song',source_album:'Old album',duration:180,unavailable:false}]})`,
    c,
  );
  assert.match(html, /publicada en Old album/);
  assert.match(html, /guardará en New album/);
});
test("unavailable videos cannot be selected in the interface", () => {
  const c = context();
  const html = vm.runInContext(
    `albumView({title:'Album',artist:'Artist',tracks:[{number:1,title:'Song',unavailable:true,reason:'No audio official'}]})`,
    c,
  );
  assert.match(html, /type="checkbox"[^>]*disabled/);
});
test("HTML in catalogue and errors is escaped", () => {
  const c = context();
  const html = vm.runInContext(
    `state.error='<script>bad</script>';albumsView()`,
    c,
  );
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script&gt;/);
});
test("a fresh session starts with a blank search and empty selection", () => {
  const c = context();
  const html = vm.runInContext("albumsView()", c);
  assert.match(html, /id="query"[^>]*value=""/);
  assert.equal(vm.runInContext("basketSize()", c), 0);
  assert.equal(vm.runInContext("state.jobs.length", c), 0);
});
test("polling shares the pending request and reports stale data honestly", async () => {
  const c = context();
  await vm.runInContext(
    `(async()=>{let reject;globalThis.calls=0;api=()=>{calls++;return new Promise((_,r)=>reject=r)};const a=poll();await poll();reject(Error('offline'));await a;})()`,
    c,
  );
  assert.equal(c.calls, 1);
  assert.equal(vm.runInContext("$('#connection').hidden", c), false);
  assert.match(
    vm.runInContext("$('#connection').textContent", c),
    /desactualizados/,
  );
});
test("poll recovery removes stale-data warning", async () => {
  const c = context();
  await vm.runInContext(
    `(async()=>{$('#connection').hidden=false;api=async()=>[];await poll()})()`,
    c,
  );
  assert.equal(vm.runInContext("$('#connection').hidden", c), true);
});
test("song selection requires its exact audio ID in the chosen publication", () => {
  const c = context();
  const plan = vm.runInContext(
    `planDownload({tracks:[
    {number:4,video_id:'studio-audio'},
    {number:5,video_id:'other-mix'},
    {number:6,video_id:'blocked',unavailable:true}
  ]}, {songs:new Map([['studio-audio','Thriller'],['wrong-id','Beat It'],['blocked','Blocked']])})`,
    c,
  );
  assert.deepEqual(Array.from(plan.tracks), [4]);
  assert.deepEqual(Array.from(plan.missing.values()), ["Beat It", "Blocked"]);
});
test("a failed publication stays selected after the other publication queues", async () => {
  const c = context();
  await vm.runInContext(
    `(async () => {
    selection('first', 'First').songs.set('track1', 'Song 1');
    selection('second', 'Second').songs.set('track2', 'Song 2');
    api = async (path) => {
      if (path === '/albums/first') return {id:'first', tracks:[{number:1,video_id:'track1'}]};
      if (path === '/albums/second') throw Error('Offline');
      return {};
    };
    await downloadBasket();
  })()`,
    c,
  );
  assert.equal(vm.runInContext("state.basket.has('first')", c), false);
  assert.equal(vm.runInContext("state.basket.has('second')", c), true);
  assert.match(vm.runInContext("state.error", c), /Second: Offline/);
});

test("more than two tracks can be selected across releases", () => {
  const c = context();
  vm.runInContext("toast = () => {}", c);
  const checkbox = (id, releaseId) => ({
    checked: true,
    dataset: { song: id, title: id, releaseId, releaseTitle: releaseId },
  });
  const first = checkbox("song1", "album1");
  const second = checkbox("song2", "album2");
  const third = checkbox("song3", "album3");
  const change = (target) => c.listeners.change({ target });
  [first, second, third].forEach(change);
  assert.equal(third.checked, true);
  assert.equal(vm.runInContext("basketSize()", c), 3);
  first.checked = false;
  change(first);
  assert.equal(vm.runInContext("basketSize()", c), 2);
});

test("the album checkbox selects all available tracks and queues them together", async () => {
  const c = context();
  vm.runInContext(`
    state.album = {id:'a', title:'Album', tracks:[
      {number:1, video_id:'one', title:'One'},
      {number:2, video_id:'two', title:'Two'},
      {number:3, video_id:'three', title:'Three'},
      {number:4, video_id:'blocked', title:'Blocked', unavailable:true},
    ]};
    toast = () => {};
    globalThis.requests = [];
    api = async (path, body) => {
      if (path === '/albums/a') return state.album;
      if (path === '/download') requests.push(body);
      return [];
    };
  `, c);
  const master = vm.runInContext("$('#select-album')", c);
  Object.assign(master, { id: "select-album", checked: true, dataset: {} });
  const change = (target) => c.listeners.change({ target });
  change(master);
  assert.equal(master.checked, true);
  assert.equal(master.indeterminate, false);
  assert.equal(vm.runInContext("basketSize()", c), 3);

  change({ checked: false, dataset: { song: "one", releaseId: "a" } });
  assert.equal(master.checked, false);
  assert.equal(master.indeterminate, true);
  master.checked = true;
  change(master);
  master.checked = false;
  change(master);
  assert.equal(vm.runInContext("basketSize()", c), 0);
  assert.equal(master.indeterminate, false);

  master.checked = true;
  change(master);
  await vm.runInContext("downloadBasket()", c);
  assert.equal(c.requests.length, 1);
  assert.equal(c.requests[0].album_id, "a");
  assert.deepEqual(Array.from(c.requests[0].tracks), [1, 2, 3]);
  assert.equal(vm.runInContext("state.page", c), "jobs");
});

test("lyrics search and checking stay separate from downloads", async () => {
  const c = context();
  await vm.runInContext(`(async () => {
    state.page = 'lyrics';
    selection('a', 'Album').songs.set('track1', 'Song');
    const track = { id: 'abcdefghijk', title: '<Canción>', artist: 'Artista' };
    globalThis.requests = [];
    api = async (path) => {
      requests.push(path);
      return path.startsWith('/lyrics/search')
        ? [track]
        : { lyrics: 'Primera línea\\n<Segunda línea>', lyrics_status: 'Disponible' };
    };
    await findLyrics('tema & artista');
    await findLyrics(state.lyrics.query, state.lyrics.results[0]);
  })()`, c);
  assert.deepEqual(Array.from(c.requests), [
    "/lyrics/search?q=tema%20%26%20artista",
    "/lyrics/abcdefghijk",
  ]);
  const html = vm.runInContext("lyricsView()", c);
  assert.match(html, /&lt;Canción&gt;/);
  assert.match(html, /Primera línea\n&lt;Segunda línea&gt;/);
  assert.equal(vm.runInContext("basketSize()", c), 1);
});
