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
  selected: new Set(),
  jobs: [],
  request: 0,
  busy: false,
  error: "",
  searched: false,
  jobsSignature: "",
  polling: false,
  queueing: false,
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
    settings: settingsView,
  };
  $("#main").innerHTML = views[state.page]();
  if (state.page === "albums" && state.album) updateSelection();
}
function selection(id, title) {
  if (!state.basket.has(id)) {
    state.basket.set(id, {
      title,
      whole: false,
      songs: new Map(),
    });
  }
  return state.basket.get(id);
}

function cleanSelection(id) {
  const entry = state.basket.get(id);
  if (entry && !entry.whole && !entry.songs.size) state.basket.delete(id);
}

function basketLabel() {
  if (state.queueing) return "Preparando…";
  const count = state.basket.size;
  if (!count) return "Descargar selección";
  return `Descargar selección · ${count} ${count === 1 ? "publicación" : "publicaciones"}`;
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
  const entry = state.basket.get(release.id);
  return `<article class="release-card">
    <button class="album-card" data-album="${esc(release.id)}">
      ${cover(release)}
      <h3>${esc(release.title)}</h3>
      <p>${esc(release.artist)}</p>
      <p class="muted">${[release.year, release.type || "Álbum"].filter(Boolean).map(esc).join(" · ")}</p>
    </button>
    <label class="release-check">
      <input type="checkbox" data-release="${esc(release.id)}" data-title="${esc(release.title)}"
        aria-label="Seleccionar publicación ${esc(release.title)}" ${entry?.whole ? "checked" : ""}>
      Completo
    </label>
  </article>`;
}

function songRow(song) {
  const entry = state.basket.get(song.album_id);
  return `<div class="song-row">
    <input type="checkbox" data-song="${esc(song.id)}" data-release-id="${esc(song.album_id)}"
      data-title="${esc(song.title)}" data-release-title="${esc(song.album)}"
      aria-label="Seleccionar ${esc(song.title)}" ${entry?.songs.has(song.id) || entry?.whole ? "checked" : ""} ${entry?.whole ? "disabled" : ""}>
    ${cover(song, "song-cover")}
    <div><strong>${esc(song.title)}</strong><br><small>${esc(song.album || song.artist)}</small></div>
    <span class="duration">${time(song.duration)}</span>
  </div>`;
}

function artistView(artist) {
  const tabs = {
    songs: "Canciones",
    albums: "Álbumes",
    singles: "Sencillos y EP",
  };
  const tab = state.artistTab;
  const albums = artist.releases.filter(
    (release) => !/^(single|ep)$/i.test(release.type),
  );
  const singles = artist.releases.filter((release) =>
    /^(single|ep)$/i.test(release.type),
  );
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
        <p class="muted">Explora artistas y elige canciones o publicaciones para guardar.</p>
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
        !artists && !releases
          ? state.searched && !state.error
            ? '<div class="empty">No encontramos resultados para esa búsqueda.</div>'
            : '<p class="search-help">Por ejemplo, Michael Jackson o Soda Stereo.</p>'
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
    <input type="checkbox" data-track="${track.number}" aria-label="Seleccionar ${esc(track.title)}"
      ${state.selected.has(track.number) ? "checked" : ""} ${track.unavailable ? "disabled" : ""}>
    <span class="number">${String(track.number).padStart(2, "0")}</span>
    <div><strong>${esc(track.title)}</strong>${hint ? `<small>${hint}</small>` : ""}</div>
    <span class="duration">${time(track.duration)}</span>
  </div>`;
}

function albumView(album) {
  const versions = album.other_versions || [];
  const available = album.tracks.some((track) => !track.unavailable);
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
      <label><input type="checkbox" id="all" ${available ? "" : "disabled"}> Seleccionar disponibles</label>
      ${basketButton()}
    </div>
    <div class="track-list">${album.tracks.map((track) => trackRow(track, album)).join("")}</div>
    <p class="note">Destino: ${esc(state.config.directory)} · ${state.config.format === "mp3" ? "MP3 320 kbps" : "M4A, audio original"}</p>`;
}

function updateSelection() {
  const total = state.album.tracks.filter((track) => !track.unavailable).length;
  const selectAll = $("#all");
  selectAll.checked = total > 0 && state.selected.size === total;
  selectAll.indeterminate =
    state.selected.size > 0 && state.selected.size < total;
  updateBasketButton();
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
    state.artistTab = "songs";
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
    state.selected = new Set(planDownload(a, state.basket.get(a.id)).tracks);
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
function saveAlbumSelection() {
  const album = state.album;
  const entry = selection(album.id, album.title);
  entry.whole = false;
  for (const track of album.tracks) {
    if (track.unavailable) continue;
    if (state.selected.has(track.number))
      entry.songs.set(track.video_id, track.title);
    else entry.songs.delete(track.video_id);
  }
  cleanSelection(album.id);
}

function planDownload(album, entry) {
  if (!entry) return { tracks: [], missing: new Map() };
  const available = album.tracks.filter((track) => !track.unavailable);
  const missing = new Map(entry.songs);
  const tracks = available
    .filter((track) => {
      const selectedSong = missing.delete(track.video_id);
      return entry.whole || selectedSong;
    })
    .map((track) => track.number);
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
            whole: false,
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
    if (state.album) {
      state.selected = new Set(
        planDownload(state.album, state.basket.get(state.album.id)).tracks,
      );
    }
    render();
  }
}

document.addEventListener("change", (e) => {
  const { target } = e;
  if (
    !(
      target.dataset.release ||
      target.dataset.song ||
      target.dataset.track ||
      target.id === "all"
    )
  )
    return;
  if (state.queueing) {
    render();
    return;
  }
  if (target.dataset.release) {
    const id = target.dataset.release;
    selection(id, target.dataset.title).whole = target.checked;
    cleanSelection(id);
    updateBasketButton();
  }
  if (target.dataset.song) {
    const id = target.dataset.releaseId;
    const songs = selection(id, target.dataset.releaseTitle).songs;
    if (target.checked) songs.set(target.dataset.song, target.dataset.title);
    else songs.delete(target.dataset.song);
    cleanSelection(id);
    updateBasketButton();
  }
  if (target.dataset.track) {
    const number = Number(target.dataset.track);
    if (target.checked) state.selected.add(number);
    else state.selected.delete(number);
    saveAlbumSelection();
    updateSelection();
  }
  if (target.id === "all") {
    state.selected = new Set(
      target.checked
        ? state.album.tracks
            .filter((track) => !track.unavailable)
            .map((track) => track.number)
        : [],
    );
    saveAlbumSelection();
    render();
  }
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
    if (b.id === "stop") {
      await api("/shutdown", {});
      $("#connection").hidden = false;
      $("#connection").textContent =
        "Surco se cerró. Las descargas activas quedan en pausa.";
      clearInterval(poller);
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
let poller;
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
        "Faltan componentes de audio. Sigue los pasos de instalación del README.",
      );
    render();
    await poll();
    poller = setInterval(() => {
      poll();
    }, 1500);
  } catch (e) {
    $("#main").innerHTML =
      `<h1>No se pudo abrir Surco</h1><p class="error">${esc(e.message)}</p>`;
  }
}
init();
