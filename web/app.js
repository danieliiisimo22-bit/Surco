"use strict";
const $ = (s) => document.querySelector(s),
  esc = (v) =>
    String(v ?? "").replace(
      /[&<>"']/g,
      (c) =>
        ({
          "&": "&amp;",
          "<": "&lt;",
          ">": "&gt;",
          '"': "&quot;",
          "'": "&#39;",
        })[c],
    );
const state = {
  page: "albums",
  token: "",
  config: null,
  results: [],
  artists: [],
  artist: null,
  artistTab: "songs",
  basket: new Map(),
  query: "",
  album: null,
  jobs: [],
  request: 0,
  busy: false,
  error: "",
  searched: false,
  jobsSignature: "",
  polling: false,
  queueing: false,
  lyrics: { query: "", results: null, song: null, result: null, busy: false, error: "", request: 0 },
};
const label = {
  queued: "En espera",
  running: "En curso",
  paused: "En pausa",
  cancelled: "Cancelada",
  completed: "Completada",
  attention: "Con pendientes",
  pending: "Pendiente",
  checking: "Comprobando",
  downloading: "Descargando",
  converting: "Preparando formato",
  tagging: "Etiquetando",
  saved: "Guardada",
  missing: "Archivo no localizado",
  error: "No guardada",
};
function toast(message, bad = false) {
  const el = document.createElement("div");
  el.className = "toast" + (bad ? " bad" : "");
  el.textContent = message;
  $("#toasts").append(el);
  setTimeout(() => el.remove(), bad ? 12000 : 5000);
}
async function api(path, body) {
  const controller = new AbortController(),
    timer = setTimeout(() => controller.abort(), 110000);
  try {
    const r = await fetch("/api" + path, {
      method: body === undefined ? "GET" : "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Surco-Token": state.token,
      },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: controller.signal,
    });
    const data = await r.json();
    if (!r.ok) throw Error(data.error || "No se pudo completar la operación.");
    return data;
  } catch (e) {
    if (e.name === "AbortError")
      throw Error("La consulta tardó demasiado. Inténtalo de nuevo.");
    throw e;
  } finally {
    clearTimeout(timer);
  }
}
function cover(a, cls = "cover") {
  return a.image
    ? `<img class="${cls}" src="${esc(a.image)}" alt="Portada de ${esc(a.title)}" loading="lazy" referrerpolicy="no-referrer">`
    : `<div class="${cls}" aria-hidden="true"></div>`;
}
function time(seconds) {
  if (!seconds) return "—";
  const rounded = Math.round(seconds);
  return `${Math.floor(rounded / 60)}:${String(rounded % 60).padStart(2, "0")}`;
}
function render() {
  document.querySelectorAll("nav [data-page]").forEach((button) => {
    const active = button.dataset.page === state.page;
    button.classList.toggle("active", active);
    if (active) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  });
  const views = {
    albums: albumsView,
    jobs: jobsView,
    lyrics: lyricsView,
    settings: settingsView,
  };
  $("#main").innerHTML = views[state.page]();
  if (state.page === "albums" && state.album) syncAlbumSelection();
}
function selection(id, title) {
  if (!state.basket.has(id)) {
    state.basket.set(id, {
      title,
      songs: new Map(),
    });
  }
  return state.basket.get(id);
}

function cleanSelection(id) {
  const entry = state.basket.get(id);
  if (entry && !entry.songs.size) state.basket.delete(id);
}

function basketSize() {
  return Array.from(state.basket.values()).reduce((count, entry) => count + entry.songs.size, 0);
}

function basketLabel() {
  if (state.queueing) return "Preparando…";
  return `Descargar selección · ${basketSize()}`;
}

function basketButton() {
  return `<button class="primary" id="download" ${!state.basket.size || state.queueing ? "disabled" : ""}>${basketLabel()}</button>`;
}

function updateBasketButton() {
  const button = $("#download");
  if (button) {
    button.disabled = state.queueing || !state.basket.size;
    button.textContent = basketLabel();
  }
}

function releaseCard(release) {
  return `<button class="album-card" data-album="${esc(release.id)}">
      ${cover(release)}
      <h3>${esc(release.title)}</h3>
      <p>${esc(release.artist)}</p>
      <p class="muted">${[release.year, release.type || "Álbum"].filter(Boolean).map(esc).join(" · ")}</p>
    </button>`;
}

function songCheckbox(song, album) {
  const checked = state.basket.get(album.id)?.songs.has(song.id);
  return `<input type="checkbox" data-song="${esc(song.id)}" data-release-id="${esc(album.id)}"
      data-title="${esc(song.title)}" data-release-title="${esc(album.title)}"
      aria-label="Seleccionar ${esc(song.title)}" ${checked ? "checked" : ""} ${song.unavailable || state.queueing ? "disabled" : ""}>`;
}

function songRow(song) {
  return `<div class="song-row">
    ${songCheckbox(song, { id: song.album_id, title: song.album })}
    ${cover(song, "song-cover")}
    <div><strong>${esc(song.title)}</strong><br><small>${esc(song.album || song.artist)}</small></div>
    <span class="duration">${time(song.duration)}</span>
  </div>`;
}

function isSingle(release) {
  return /^(single|ep)$/i.test(release.type);
}

function artistView(artist) {
  const tabs = {
    songs: "Canciones",
    albums: "Álbumes",
    singles: "Sencillos y EP",
  };
  const tab = state.artistTab;
  const albums = artist.releases.filter((release) => !isSingle(release));
  const singles = artist.releases.filter(isSingle);
  const rows =
    tab === "songs" ? artist.songs : tab === "albums" ? albums : singles;
  const cards = rows.map(tab === "songs" ? songRow : releaseCard).join("");
  return `
    <button class="subtle back" data-action="artist-back">← Resultados</button>
    <section class="artist-heading">${cover(artist, "artist-cover")}<h1>${esc(artist.title)}</h1></section>
    <div class="catalog-toolbar">
      <div class="artist-tabs" role="group" aria-label="Catálogo de artista">
        ${Object.entries(tabs)
          .map(
            ([id, title]) =>
              `<button data-artist-tab="${id}" aria-pressed="${id === tab}">${title}</button>`,
          )
          .join("")}
      </div>
      ${basketButton()}
    </div>
    ${state.error ? `<p class="error" role="alert">${esc(state.error)}</p>` : ""}
    ${cards ? `<div class="${tab === "songs" ? "track-list" : "grid"}">${cards}</div>` : '<div class="empty">No hay publicaciones disponibles en esta sección.</div>'}`;
}

function albumsView() {
  if (state.album) return albumView(state.album);
  if (state.artist) return artistView(state.artist);
  const artists = state.artists
    .map(
      (artist) => `
    <button class="artist-card" data-artist="${esc(artist.id)}">
      ${cover(artist, "artist-cover")}<strong>${esc(artist.title)}</strong>
    </button>`,
    )
    .join("");
  const releases = state.results.map(releaseCard).join("");
  return `
    ${
      !state.searched
        ? `<section class="intro">
      <div>
        <h1>Buscar música</h1>
        <p class="muted">Explora artistas y álbumes.</p>
      </div>
      <div class="record" aria-hidden="true"></div>
    </section>`
        : "<h1>Buscar música</h1>"
    }
    <form id="search" class="search glass" data-lens role="search">
      <input id="query" aria-label="Artista, álbum o enlace de YouTube Music" type="search"
        value="${esc(state.query)}" placeholder="Artista, álbum o enlace de YouTube Music" required>
      <button class="primary" ${state.busy ? "disabled" : ""}>${state.busy ? "Buscando…" : "Buscar"}</button>
    </form>
    ${state.error ? `<p class="error" role="alert">${esc(state.error)}</p>` : ""}
    ${
      state.busy
        ? '<p class="search-help" role="status">Consultando el catálogo…</p>'
        : `
      ${artists || releases || state.basket.size ? `<div class="catalog-toolbar">${basketButton()}</div>` : ""}
      ${artists ? `<h2>Artistas</h2><div class="artists">${artists}</div>` : ""}
      ${releases ? `<h2>Publicaciones</h2><div class="grid">${releases}</div>` : ""}
      ${
        !artists && !releases && state.searched && !state.error
          ? '<div class="empty">No encontramos resultados para esa búsqueda.</div>'
          : ""
      }
    `
    }`;
}
function trackRow(track, album) {
  const hint = track.unavailable
    ? esc(track.reason)
    : track.source_album && track.source_album !== album.title
      ? `La grabación también está publicada en ${esc(track.source_album)}. Se guardará en ${esc(album.title)}.`
      : "";
  return `<div class="track-row ${track.unavailable ? "blocked" : ""}">
    ${songCheckbox({ ...track, id: track.video_id }, album)}
    <span class="number">${String(track.number).padStart(2, "0")}</span>
    <div><strong>${esc(track.title)}</strong>${hint ? `<small>${hint}</small>` : ""}</div>
    <span class="duration">${time(track.duration)}</span>
  </div>`;
}

function albumView(album) {
  const versions = album.other_versions || [];
  return `<button class="subtle back" data-action="back">← Volver a resultados</button>
    <section class="album-top">${cover(album)}<div>
      <h1>${esc(album.title)}</h1>
      <p class="muted">${[album.artist, album.year, `${album.tracks.length} pistas`].filter(Boolean).map(esc).join(" · ")}</p>
    </div></section>
    ${
      versions.length
        ? `<details class="editions"><summary>Otras ediciones</summary>
      <div class="actions">${versions.map((version) => `<button data-album="${esc(version.id)}">${esc(version.title)}</button>`).join("")}</div>
    </details>`
        : ""
    }
    ${state.error ? `<p class="error" role="alert">${esc(state.error)}</p>` : ""}
    <div class="track-toolbar">
      <label class="album-selection">
        <input type="checkbox" id="select-album" ${state.queueing || album.tracks.every((track) => track.unavailable) ? "disabled" : ""}>
        Seleccionar álbum completo
      </label>
      ${basketButton()}
    </div>
    <div class="track-list" id="album-tracks">${album.tracks.map((track) => trackRow(track, album)).join("")}</div>
    <p class="note">Destino: ${esc(state.config.directory)} · ${state.config.format === "mp3" ? "MP3 320 kbps" : "M4A, audio original"}</p>`;
}

function syncAlbumSelection() {
  const album = state.album;
  const master = $("#select-album");
  const songs = state.basket.get(album.id)?.songs;
  const available = album.tracks.filter((track) => !track.unavailable);
  const selected = available.filter((track) => songs?.has(track.video_id)).length;
  master.checked = selected > 0 && selected === available.length;
  master.indeterminate = selected > 0 && selected < available.length;
  document.querySelectorAll("#album-tracks input[data-song]").forEach((input) => {
    if (!input.disabled) input.checked = songs?.has(input.dataset.song) ?? false;
  });
}

function jobOutcome(item) {
  const error = item.status === "error";
  const needsSession = error && /sign in|log in|bot|cookies/i.test(item.detail);
  return `
    <div class="outcome">
      <strong>${esc(item.track.number)}. ${esc(item.track.title)}</strong>
      <p><span class="status ${error ? "attention" : ""}">${esc(label[item.status] || item.status)}</span>
        · ${esc(item.detail || "En espera")}</p>
      ${item.lyrics ? `<p>Letra: ${esc(item.lyrics)}</p>` : ""}
      ${item.path ? `<div class="path">Archivo: ${esc(item.path)}</div>` : ""}
      ${item.backup ? `<div class="path">Respaldo: ${esc(item.backup)}</div>` : ""}
      ${needsSession ? "<p>La fuente pidió autenticación. En Ajustes puedes elegir tu navegador con la sesión iniciada y reintentar esta pista.</p>" : ""}
    </div>`;
}

function jobCard(job) {
  const id = esc(job.id);
  const saved = job.items.filter((item) => item.status === "saved").length;
  const active = ["queued", "running"].includes(job.status);
  const paused = job.status === "paused";
  const canResume = paused || ["attention", "cancelled"].includes(job.status);
  return `
    <article class="job" data-job-card="${id}">
      <div class="job-head">
        ${cover(job.album, "job-cover")}
        <div class="job-info">
          <h3>${esc(job.album.title)}</h3>
          <p>${esc(job.album.artist)} · ${esc(job.config.format.toUpperCase())}</p>
          <span class="status ${esc(job.status)}">${esc(label[job.status] || job.status)} · ${saved} de ${job.items.length} guardadas</span>
        </div>
        <div class="actions">
          ${active ? `<button data-control="pause" data-job="${id}">Pausar</button>` : ""}
          ${canResume ? `<button data-control="resume" data-job="${id}">${paused ? "Continuar" : "Reintentar pendientes"}</button>` : ""}
          ${active || paused ? `<button class="subtle" data-control="cancel" data-job="${id}">Cancelar</button>` : ""}
          ${saved ? `<button data-folder="${id}">Abrir carpeta ↗</button>` : ""}
        </div>
      </div>
      <progress class="progress" value="${saved}" max="${job.items.length}" aria-label="Pistas guardadas"></progress>
      <details data-details="${id}">
        <summary>Resultados por canción</summary>
        ${job.items.map(jobOutcome).join("")}
        <p><a href="/api/report/${encodeURIComponent(job.id)}" download>Guardar informe del álbum</a></p>
      </details>
    </article>`;
}

function jobsView() {
  const cards = state.jobs.map(jobCard).join("");
  return `<h1>Descargas</h1>${cards || '<div class="empty"><strong>Aún no hay descargas</strong>Elige un álbum para empezar.</div>'}`;
}

function lyricsView() {
  const { query, results, song, result, busy, error } = state.lyrics;
  const songs = results?.map((track) => `
    <div class="lyric-result">
      ${cover(track, "song-cover")}
      <div><strong>${esc(track.title)}</strong><br><small>${[track.artist, track.album].filter(Boolean).map(esc).join(" · ")}</small></div>
      <button data-lyric="${esc(track.id)}" aria-pressed="${song?.id === track.id}">Comprobar letra</button>
    </div>`).join("");
  const detail = song ? `
    <section class="lyric-sheet" aria-labelledby="lyric-title">
      <h2 id="lyric-title">${esc(song.title)}</h2>
      <p class="muted">${esc(song.artist)}</p>
      ${result ? `<p role="status">${esc(result.lyrics_status)}</p>` : ""}
      ${result?.lyrics ? `<p class="lyric-text">${esc(result.lyrics)}</p>` : ""}
    </section>` : "";
  return `<h1>Letras</h1>
    <form id="lyrics-search" class="search glass" data-lens role="search">
      <input id="lyric-query" type="search" aria-label="Canción o artista"
        value="${esc(query)}" placeholder="Canción o artista" required maxlength="500">
      <button class="primary">Buscar</button>
    </form>
    ${busy ? `<p class="search-help" role="status">${song ? "Comprobando letra…" : "Buscando canciones…"}</p>` : ""}
    ${error ? `<p class="error" role="alert">${esc(error)}</p>` : ""}
    ${detail}
    ${songs ? `<div class="track-list">${songs}</div>` : results && !busy && !error ? '<div class="empty">No encontramos canciones.</div>' : ""}
  `;
}

async function findLyrics(query, song = null) {
  const view = state.lyrics;
  const request = ++view.request;
  if (!song) {
    view.query = query;
    view.results = null;
  }
  view.song = song;
  view.result = null;
  view.error = "";
  view.busy = true;
  render();
  try {
    const path = song ? `/lyrics/${encodeURIComponent(song.id)}` : `/lyrics/search?q=${encodeURIComponent(query)}`;
    const data = await api(path);
    if (request !== view.request) return;
    if (song) view.result = data;
    else view.results = data;
  } catch (error) {
    if (request === view.request) view.error = error.message;
  } finally {
    if (request === view.request) {
      view.busy = false;
      if (state.page === "lyrics") {
        render();
        if (song) $("#lyric-title")?.scrollIntoView({ block: "nearest" });
      }
    }
  }
}

function settingsView() {
  const config = state.config;
  const browsers = [
    ["", "Sin sesión"],
    ["firefox", "Firefox"],
    ["edge", "Edge"],
    ["chrome", "Chrome"],
  ];
  const options = browsers
    .map(
      ([value, name]) =>
        `<option value="${value}" ${config.browser === value ? "selected" : ""}>${name}</option>`,
    )
    .join("");
  return `
    <div class="settings">
      <h1>Ajustes</h1>
      <form id="settings">
        <div class="field">
          <label for="directory">Carpeta de música</label>
          <input id="directory" value="${esc(config.directory)}" required>
          <small>Se organizará como Artista / Año - Álbum / Número - Canción.</small>
        </div>
        <div class="field">
          <label for="format">Formato</label>
          <select id="format">
            <option value="mp3" ${config.format === "mp3" ? "selected" : ""}>MP3 · 320 kbps</option>
            <option value="m4a" ${config.format === "m4a" ? "selected" : ""}>M4A · audio AAC original</option>
          </select>
        </div>
        <details>
          <summary>Acceso a fuentes que requieren sesión</summary>
          <div class="field">
            <label for="browser">Navegador con tu sesión de YouTube</label>
            <select id="browser">${options}</select>
          </div>
        </details>
        <button class="primary">Guardar ajustes</button>
      </form>
    </div>`;
}
async function search(q) {
  const id = ++state.request;
  state.query = q;
  state.artist = null;
  state.artists = [];
  state.results = [];
  state.album = null;
  state.busy = true;
  state.error = "";
  state.searched = true;
  render();
  try {
    const data = await api("/search?q=" + encodeURIComponent(q));
    if (id !== state.request || state.page !== "albums") return;
    state.results = data.releases || [];
    state.artists = data.artists || [];
  } catch (e) {
    if (id === state.request) state.error = e.message;
  } finally {
    if (id === state.request) {
      state.busy = false;
      render();
    }
  }
}
async function openArtist(id) {
  const serial = ++state.request;
  state.busy = true;
  state.error = "";
  render();
  try {
    const artist = await api("/artists/" + encodeURIComponent(id));
    if (serial !== state.request || state.page !== "albums") return;
    state.artist = artist;
    if (artist.songs.length) state.artistTab = "songs";
    else if (artist.releases.some((release) => !isSingle(release))) state.artistTab = "albums";
    else state.artistTab = artist.releases.length ? "singles" : "songs";
  } catch (error) {
    if (serial === state.request) state.error = error.message;
  } finally {
    if (serial === state.request) {
      state.busy = false;
      render();
    }
  }
}

async function openAlbum(id) {
  const serial = ++state.request;
  state.busy = true;
  state.error = "";
  $("#main").innerHTML =
    '<button class="subtle" data-action="back">← Volver</button><p role="status">Abriendo la lista de audio del álbum…</p>';
  try {
    const a = await api("/albums/" + encodeURIComponent(id));
    if (serial !== state.request || state.page !== "albums") return;
    state.album = a;
  } catch (e) {
    if (serial === state.request) state.error = e.message;
  } finally {
    if (serial === state.request) {
      state.busy = false;
      render();
    }
  }
}
async function poll() {
  if (state.polling) return;
  state.polling = true;
  try {
    const jobs = await api("/jobs");
    state.jobs = jobs;
    $("#connection").hidden = true;
    $("#badge").textContent =
      jobs.filter((j) => ["queued", "running"].includes(j.status)).length || "";
    const signature = JSON.stringify(jobs);
    if (state.page === "jobs" && signature !== state.jobsSignature) {
      const expanded = new Set(
        [...document.querySelectorAll("details[open]")].map(
          (e) => e.dataset.details,
        ),
      );
      render();
      document
        .querySelectorAll("[data-details]")
        .forEach((e) => (e.open = expanded.has(e.dataset.details)));
    }
    state.jobsSignature = signature;
  } catch (e) {
    $("#connection").hidden = false;
    $("#connection").textContent =
      "No se pudo actualizar. Los datos mostrados pueden estar desactualizados. " +
      e.message;
  } finally {
    state.polling = false;
  }
}
document.addEventListener("submit", (e) => {
  e.preventDefault();
  (async () => {
    if (e.target.id === "search") await search($("#query").value.trim());
    if (e.target.id === "lyrics-search") {
      const query = $("#lyric-query").value.trim();
      if (query) await findLyrics(query);
    }
    if (e.target.id === "settings") {
      const config = {
        directory: $("#directory").value.trim(),
        format: $("#format").value,
        browser: $("#browser").value,
      };
      await api("/settings", config);
      state.config = config;
      toast("Ajustes guardados.");
    }
  })().catch((e) => toast(e.message, true));
});
function planDownload(album, entry) {
  if (!entry) return { tracks: [], missing: new Map() };
  const available = album.tracks.filter((track) => !track.unavailable);
  const missing = new Map(entry.songs);
  const tracks = available
    .filter((track) => entry.songs.has(track.video_id))
    .map((track) => {
      missing.delete(track.video_id);
      return track.number;
    });
  return { tracks, missing };
}

async function downloadBasket() {
  if (state.queueing || !state.basket.size) return;
  state.queueing = true;
  state.error = "";
  render();
  const failures = [];
  try {
    // Process each publication once. Preserve any selection that could not be queued.
    for (const [id, entry] of [...state.basket]) {
      try {
        const album = await api("/albums/" + encodeURIComponent(id));
        const { tracks, missing } = planDownload(album, entry);
        if (tracks.length)
          await api("/download", { album_id: album.id, tracks });
        if (!tracks.length && !missing.size)
          throw Error("No hay pistas disponibles.");
        if (missing.size) {
          failures.push(
            `${entry.title}: sin audio confirmado para ${[...missing.values()].join(", ")}.`,
          );
          state.basket.set(id, {
            ...entry,
            songs: missing,
          });
        } else {
          state.basket.delete(id);
        }
      } catch (error) {
        failures.push(`${entry.title}: ${error.message}`);
      }
    }
    if (failures.length) {
      state.error = failures.join("\n");
      if (state.page !== "albums") toast(state.error, true);
    } else {
      state.album = null;
      state.page = "jobs";
      toast("Selección añadida a descargas.");
      await poll();
    }
  } finally {
    state.queueing = false;
    render();
  }
}

document.addEventListener("change", (e) => {
  const { target } = e;
  const { song, title, releaseId, releaseTitle } = target.dataset;
  if (state.queueing || target.disabled) return;
  if (target.id === "select-album" && state.album) {
    const { id, title, tracks } = state.album;
    if (target.checked) {
      const { songs } = selection(id, title);
      for (const track of tracks) {
        if (!track.unavailable) songs.set(track.video_id, track.title);
      }
    } else {
      state.basket.delete(id);
    }
  } else if (!song) {
    return;
  } else if (target.checked) {
    selection(releaseId, releaseTitle).songs.set(song, title);
  } else {
    state.basket.get(releaseId)?.songs.delete(song);
    cleanSelection(releaseId);
  }
  if (state.album) syncAlbumSelection();
  updateBasketButton();
});
document.addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b || b.disabled) return;
  (async () => {
    if (b.dataset.page) {
      state.request++;
      state.busy = false;
      state.page = b.dataset.page;
      render();
      if (state.page === "jobs") await poll();
      return;
    }
    if (b.dataset.artist) {
      await openArtist(b.dataset.artist);
      return;
    }
    if (b.dataset.artistTab) {
      state.artistTab = b.dataset.artistTab;
      render();
      document.querySelector(`[data-artist-tab="${state.artistTab}"]`)?.focus();
      return;
    }
    if (b.dataset.action === "artist-back") {
      state.artist = null;
      state.error = "";
      render();
      $("#query")?.focus();
      return;
    }
    if (b.dataset.album) {
      await openAlbum(b.dataset.album);
      return;
    }
    if (b.dataset.action === "back") {
      state.request++;
      state.busy = false;
      state.album = null;
      state.error = "";
      render();
      $("#query")?.focus();
      return;
    }
    if (b.id === "download") {
      await downloadBasket();
      return;
    }
    if (b.dataset.lyric) {
      const song = state.lyrics.results?.find((song) => song.id === b.dataset.lyric);
      if (song) await findLyrics(state.lyrics.query, song);
      return;
    }
    if (b.dataset.control) {
      b.disabled = true;
      try {
        await api("/jobs/" + b.dataset.job + "/" + b.dataset.control, {});
        await poll();
      } finally {
        b.disabled = false;
      }
      return;
    }
    if (b.id === "folder" || b.dataset.folder) {
      await api("/folder", { job_id: b.dataset.folder || null });
      return;
    }
  })().catch((e) => {
    b.disabled = false;
    toast(e.message, true);
  });
});
document.addEventListener(
  "error",
  (e) => {
    if (e.target.tagName === "IMG") {
      e.target.removeAttribute("src");
      e.target.alt = "Portada no disponible";
    }
  },
  true,
);
async function init() {
  try {
    const data = await api("/bootstrap");
    state.token = data.token;
    state.config = data.config;
    if (data.warnings?.length) {
      const notice = $("#profile-warning");
      notice.textContent = data.warnings.join(" ");
      notice.hidden = false;
    }
    if (!data.ready)
      throw Error(
        "Faltan archivos de Surco. Extrae el ZIP por completo y abre surco.exe desde esa carpeta.",
      );
    render();
    await poll();
    setInterval(() => {
      poll();
    }, 1500);
  } catch (e) {
    $("#main").innerHTML =
      `<h1>No se pudo abrir Surco</h1><p class="error">${esc(e.message)}</p>`;
  }
}
init();
