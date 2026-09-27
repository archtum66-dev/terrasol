// Prüfseite - Logik im Browser.
//
// Eigene Datei statt Inline-Script: So kann der Server eine strikte
// Content-Security-Policy (script-src 'self') setzen, die eingeschleustes
// HTML - etwa über einen präparierten Dateinamen - gar nicht erst ausführt.
//
// Regel für diese Datei: Alles, was von aussen kommt (Dateiname, Antwort des
// Servers), geht nur über textContent in die Seite. Nie über innerHTML.
'use strict';

const ablage = document.getElementById('ablage');
const datei  = document.getElementById('datei');
const lauf   = document.getElementById('lauf');
const kasten = document.getElementById('ergebnis');
const titel  = document.getElementById('titel');
const text   = document.getElementById('text');
const daten  = document.getElementById('daten');
const logo   = document.getElementById('logo');

if (logo) logo.addEventListener('error', () => { logo.style.display = 'none'; });

['dragenter', 'dragover'].forEach(e =>
  ablage.addEventListener(e, ev => { ev.preventDefault(); ablage.classList.add('drueber'); }));
['dragleave', 'drop'].forEach(e =>
  ablage.addEventListener(e, ev => { ev.preventDefault(); ablage.classList.remove('drueber'); }));
ablage.addEventListener('drop', ev => {
  const f = ev.dataTransfer.files[0];
  if (f) pruefen(f);
});
datei.addEventListener('change', () => { if (datei.files[0]) pruefen(datei.files[0]); });

/** SHA-256 im Browser. Das Dokument bleibt hier. */
async function fingerabdruck(file) {
  const puffer = await file.arrayBuffer();
  const roh = await crypto.subtle.digest('SHA-256', puffer);
  return Array.from(new Uint8Array(roh))
    .map(b => b.toString(16).padStart(2, '0')).join('');
}

/** Nur Links auf den öffentlichen Block-Explorer werden anklickbar. */
function sichererLink(url) {
  try {
    const u = new URL(url);
    return u.protocol === 'https:' && u.hostname === 'solscan.io' ? u.href : null;
  } catch {
    return null;
  }
}

/**
 * felder: [Bezeichnung, Wert] oder [Bezeichnung, Wert, Link].
 * Wert und Bezeichnung werden ausschliesslich als Text gesetzt.
 */
function zeigen(art, kopf, inhalt, felder) {
  lauf.classList.remove('zeigen');
  kasten.className = 'ergebnis zeigen ' + art;
  titel.textContent = kopf;
  text.textContent = inhalt;
  daten.replaceChildren();
  for (const [k, v, link] of felder) {
    const dt = document.createElement('dt');
    dt.textContent = k;
    const dd = document.createElement('dd');
    const ziel = link ? sichererLink(link) : null;
    if (ziel) {
      const a = document.createElement('a');
      a.href = ziel;
      a.target = '_blank';
      a.rel = 'noopener noreferrer';
      a.textContent = String(v);
      dd.appendChild(a);
    } else {
      dd.textContent = String(v);
    }
    daten.append(dt, dd);
  }
}

function groesseText(n) {
  return n < 1024 ? n + ' Bytes'
    : n < 1048576 ? (n / 1024).toFixed(1) + ' kB'
    : (n / 1048576).toFixed(1) + ' MB';
}

async function pruefen(file) {
  kasten.classList.remove('zeigen');
  lauf.classList.add('zeigen');

  const hash = await fingerabdruck(file);
  const dateiFeld = ['Datei', file.name + ' · ' + groesseText(file.size)];
  let a;
  try {
    const antwort = await fetch('/api/pruefen?hash=' + encodeURIComponent(hash));
    if (!antwort.ok) throw new Error('HTTP ' + antwort.status);
    a = await antwort.json();
  } catch (e) {
    zeigen('gelb', '⚠ Prüfung nicht möglich',
      'Der Prüfdienst ist nicht erreichbar. Bitte später erneut versuchen.',
      [['Fingerabdruck', hash]]);
    return;
  }

  switch (a.status) {
    case 'registriert': {
      const zeit = typeof a.blockTime === 'number'
        ? new Date(a.blockTime * 1000).toLocaleString('de-CH',
            { dateStyle: 'long', timeStyle: 'short' })
        : 'unbekannt';
      zeigen('gruen', '✓ Registriert und unverändert',
        'Dieses Dokument wurde bei uns registriert und ist seither Byte für Byte unverändert.',
        [
          dateiFeld,
          ['Registriert am', zeit],
          ['Fingerabdruck', hash],
          ['Beleg auf der Kette', a.signature, a.explorer],
        ]);
      return;
    }
    case 'abweichung':
      // Das Register kennt den Fingerabdruck, die Kette bestätigt ihn aber
      // nicht (anderer Hash, fremder Signierer oder gescheiterte Transaktion).
      zeigen('rot', '✗ Kein gültiger Nachweis',
        'Zu diesem Dokument gibt es einen Registereintrag, aber die Blockchain '
        + 'bestätigt ihn nicht. Der Eintrag ist kein gültiger Nachweis.',
        [dateiFeld, ['Fingerabdruck', hash]]);
      return;
    case 'kette_offline':
      zeigen('gelb', '⚠ Prüfung vorübergehend nicht möglich',
        'Das Dokument ist bei uns registriert, die Blockchain ist im Moment aber '
        + 'nicht erreichbar. Ohne Bestätigung der Kette geben wir kein Ergebnis aus.',
        [dateiFeld, ['Fingerabdruck', hash]]);
      return;
    default:
      zeigen('rot', '✗ Nicht registriert',
        'Für dieses Dokument gibt es keinen Eintrag. Entweder wurde es nie '
        + 'registriert — oder es wurde nach der Registrierung verändert. Beides '
        + 'ist von aussen nicht unterscheidbar.',
        [dateiFeld, ['Fingerabdruck', hash]]);
  }
}
