// Splash screen logic (see index.html).
const info = window.SPLASH_INFO || {};
const $ = (id) => document.getElementById(id);
if (info.name) { $('name').textContent = info.name; document.title = info.name; }
$('version').textContent = [info.version, info.build && 'build ' + info.build, info.commit].filter(Boolean).join(' · ');
$('footer').textContent = [info.copyright, 'MIT License', 'Not affiliated with Resolume BV'].filter(Boolean).join(' · ');

const action = (name) => window.__TAURI__ && window.__TAURI__.core.invoke('splash_action', { action: name });
let details = '';

window.splash = {
  step(index, total, text, warning) {
    const bar = $('bar');
    bar.setAttribute('aria-valuemax', total);
    bar.setAttribute('aria-valuenow', index);
    bar.firstElementChild.style.width = (100 * (index >= total ? 1 : (index - 1) / total)) + '%';
    if (warning) {
      const line = document.createElement('div');
      line.textContent = text;
      $('warnings').appendChild(line);
    } else {
      $('status').textContent = text + (index < total ? '…' : '');
    }
  },
  error(message, detailText, retryable) {
    details = detailText || message;
    $('error-text').textContent = message;
    $('retry').hidden = !retryable;
    document.body.classList.add('failed');
    (retryable ? $('retry') : $('quit')).focus();
  },
  reset() {
    document.body.classList.remove('failed');
    $('warnings').textContent = '';
    $('status').textContent = 'Starting…';
    $('bar').firstElementChild.style.width = '0';
  },
  fadeOut() { document.body.classList.add('fade'); },
};

$('retry').onclick = () => action('retry');
$('open-log').onclick = () => action('open_log');
$('quit').onclick = () => action('quit');
$('copy').onclick = async () => {
  try { await navigator.clipboard.writeText(details); }
  catch {
    const t = document.createElement('textarea'); t.value = details; document.body.appendChild(t);
    t.select(); document.execCommand('copy'); t.remove();
  }
  $('copy').textContent = 'Copied';
  setTimeout(() => ($('copy').textContent = 'Copy details'), 1500);
};
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && document.body.classList.contains('failed')) action('quit');
});
