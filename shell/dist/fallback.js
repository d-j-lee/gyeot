// 셸에 든 대체 화면: 데몬을 깨우는 동안, 또는 연결하지 못했을 때만 보인다.
// 데몬이 서빙하는 얼굴과 같은 약속(set_hit_rect)으로 클릭 영역을 알린다.
(function () {
  'use strict';

  var tauri = window.__TAURI__;
  var invoke = tauri && tauri.core && typeof tauri.core.invoke === 'function' ? tauri.core.invoke : null;

  function call(cmd, args) {
    if (!invoke) return Promise.resolve();
    return invoke(cmd, args).catch(function () {
      /* 셸이 모르는 명령이면 조용히 넘어간다 */
    });
  }

  var params = new URLSearchParams(window.location.search);
  var isError = params.get('state') === 'error';
  var card = document.getElementById('card');

  if (isError) {
    document.body.classList.add('error');
    document.getElementById('loading').hidden = true;
    document.getElementById('error').hidden = false;
    // 이유는 글자로만 넣는다(HTML로 해석하지 않음).
    document.getElementById('reason').textContent = params.get('reason') || '';
  }

  var last = '';
  function report() {
    var r = card.getBoundingClientRect();
    var rect = {
      x: Math.max(0, Math.floor(r.left)),
      y: Math.max(0, Math.floor(r.top)),
      width: Math.ceil(r.width),
      height: Math.ceil(r.height),
    };
    var key = rect.x + ',' + rect.y + ',' + rect.width + ',' + rect.height;
    if (key === last) return;
    last = key;
    call('set_hit_rect', rect);
  }

  if (typeof ResizeObserver === 'function') new ResizeObserver(report).observe(card);
  window.addEventListener('resize', report);
  report();

  function wire(id, restart) {
    var btn = document.getElementById(id);
    btn.addEventListener('click', function () {
      document.getElementById('retry').disabled = true;
      document.getElementById('restart').disabled = true;
      document.getElementById('reason').textContent = restart ? '데몬을 다시 시작하는 중…' : '다시 연결하는 중…';
      call('retry_connect', { restart: restart });
    });
  }
  wire('retry', false);
  wire('restart', true);
})();
