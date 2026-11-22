// 브라우저 WebRTC 송출(WHIP)·시청(WHEP).
// WASM(Rust) 쪽에서 document::eval 로 window.live 의 함수를 부른다.
// 신호(SDP)는 API 서버로 한 번만 보낸다: ICE 후보를 다 모은 뒤 offer 를 보내므로 trickle ICE 가 필요 없다.
window.live = (() => {
  let preview = null;   // { stream, facing }
  let publisher = null; // { pc, endUrl, key, onPageHide }
  let viewer = null;    // { pc }

  function waitIceGathering(pc) {
    if (pc.iceGatheringState === 'complete') return Promise.resolve();
    return new Promise((resolve) => {
      // 후보 수집이 오래 걸리는 네트워크에서도 모은 데까지만 보내고 진행한다
      const timer = setTimeout(resolve, 3000);
      pc.addEventListener('icegatheringstatechange', () => {
        if (pc.iceGatheringState === 'complete') { clearTimeout(timer); resolve(); }
      });
    });
  }

  async function negotiate(pc, url, key) {
    await pc.setLocalDescription(await pc.createOffer());
    await waitIceGathering(pc);
    const headers = { 'Content-Type': 'application/sdp' };
    if (key) headers.Authorization = 'Bearer ' + key;
    const res = await fetch(url, { method: 'POST', headers, body: pc.localDescription.sdp });
    if (!res.ok) {
      let detail = null;
      try { detail = (await res.json()).detail; } catch (_) {}
      throw new Error(detail || `연결 실패 (${res.status})`);
    }
    await pc.setRemoteDescription({ type: 'answer', sdp: await res.text() });
  }

  // 연결 상태를 'connected' | 'unstable'(잠깐 끊김, 회복될 수 있음) | 'disconnected' 로 알려준다
  function watchConnection(pc, onState) {
    pc.addEventListener('connectionstatechange', () => {
      const s = pc.connectionState;
      if (s === 'connected') onState('connected');
      else if (s === 'disconnected') onState('unstable');
      else if (s === 'failed' || s === 'closed') onState('disconnected');
    });
  }

  function cameraError(e) {
    if (!navigator.mediaDevices || !window.isSecureContext) {
      return 'HTTPS 주소로 접속해야 카메라·마이크를 쓸 수 있습니다.';
    }
    switch (e && e.name) {
      case 'NotAllowedError': return '카메라·마이크 권한이 거부되었습니다. 브라우저 설정에서 이 사이트의 카메라·마이크를 허용해주세요.';
      case 'NotFoundError': return '카메라나 마이크를 찾을 수 없습니다.';
      case 'NotReadableError': return '다른 앱이 카메라·마이크를 쓰고 있습니다.';
      default: return '카메라·마이크를 켜지 못했습니다: ' + (e && e.message);
    }
  }

  function stopTracks(stream) {
    if (stream) stream.getTracks().forEach((t) => t.stop());
  }

  return {
    // 카메라·마이크를 켜서 videoId 요소에 미리보기를 띄운다. facing: 'user'(전면) | 'environment'(후면)
    async startPreview(videoId, facing) {
      if (!navigator.mediaDevices || !window.isSecureContext) throw new Error(cameraError());
      const open = (f) => navigator.mediaDevices.getUserMedia({
        video: { facingMode: f, width: { ideal: 1280 }, height: { ideal: 720 } },
        audio: { echoCancellation: true, noiseSuppression: true },
      });
      // 휴대폰은 카메라를 한 번에 하나만 열 수 있어서, 켜져 있는 카메라를 먼저 꺼야 다른 카메라가 열린다
      const previous = preview && preview.facing;
      this.stopPreview();
      let stream;
      try {
        stream = await open(facing);
      } catch (e) {
        // 바꾸려던 카메라가 안 열리면 원래 카메라로 되돌려 미리보기가 꺼진 채로 남지 않게 한다
        if (previous) {
          try {
            const back = await open(previous);
            preview = { stream: back, facing: previous };
            document.getElementById(videoId).srcObject = back;
          } catch (_) {}
        }
        throw new Error(cameraError(e));
      }
      preview = { stream, facing };
      const video = document.getElementById(videoId);
      video.srcObject = stream;
      video.muted = true; // 내 목소리가 스피커로 다시 나오지 않게
      await video.play().catch(() => {});
    },

    stopPreview() {
      stopTracks(preview && preview.stream);
      preview = null;
    },

    // 미리보기 중인 카메라·마이크를 송출한다
    async publish(whipUrl, endUrl, key, onState) {
      if (!preview) throw new Error('카메라를 먼저 켜주세요.');
      const pc = new RTCPeerConnection();
      preview.stream.getTracks().forEach((track) => pc.addTransceiver(track, { direction: 'sendonly' }));
      watchConnection(pc, onState);
      // 탭을 닫거나 다른 페이지로 가면 방송을 끝낸다 (keepalive: 페이지가 닫혀도 요청은 끝까지 간다)
      const onPageHide = () => this.stopPublish(true);
      window.addEventListener('pagehide', onPageHide);
      publisher = { pc, endUrl, onPageHide };
      try {
        await negotiate(pc, whipUrl, key);
      } catch (e) {
        // 연결만 정리하고 미리보기는 남겨서 다시 시작할 수 있게 한다
        window.removeEventListener('pagehide', onPageHide);
        pc.close();
        publisher = null;
        throw e;
      }
    },

    // sendEnd: 서버에 종료 요청까지 보낼지 (화면을 떠날 때)
    stopPublish(sendEnd) {
      if (!publisher) return;
      const { pc, endUrl, onPageHide } = publisher;
      publisher = null;
      window.removeEventListener('pagehide', onPageHide);
      pc.close();
      if (sendEnd) {
        // 같은 출처 요청이라 로그인 쿠키가 함께 간다. 서버가 방송 소유자인지 확인한다.
        fetch(endUrl, { method: 'POST', keepalive: true }).catch(() => {});
      }
      this.stopPreview();
    },

    // 라이브를 videoId 요소에서 재생한다
    async play(videoId, whepUrl, onState) {
      this.stopPlay();
      const pc = new RTCPeerConnection();
      pc.addTransceiver('video', { direction: 'recvonly' });
      pc.addTransceiver('audio', { direction: 'recvonly' });
      const stream = new MediaStream();
      pc.addEventListener('track', (e) => stream.addTrack(e.track));
      const video = document.getElementById(videoId);
      video.srcObject = stream;
      watchConnection(pc, onState);
      viewer = { pc };
      await negotiate(pc, whepUrl, null);
      // 자동 재생 정책 때문에 소리 없이 시작한다. 컨트롤에서 음소거를 풀 수 있다.
      video.muted = true;
      await video.play().catch(() => {});
    },

    stopPlay() {
      if (!viewer) return;
      viewer.pc.close();
      viewer = null;
    },
  };
})();
