// 데스크톱 앱의 라이브 재생(WHEP). 신호(SDP)는 웹뷰가 직접 서버로 보내지 않고 앱(Rust)에 넘긴다:
//   웹뷰 → app.send('offer:<sdp>') → 앱이 서버로 POST → app.recv() 로 answer (실패하면 'error:<메시지>')
// app 은 이 재생을 시작한 eval 의 dioxus 채널이다(eval 마다 다르므로 재생할 때 넘겨받는다).
// 연결 상태는 'state:<connectionState>' 로 알린다. ICE 후보를 다 모은 뒤 offer 를 보내므로 trickle ICE 가 필요 없다.
window.nadriLive = window.nadriLive || (() => {
  let pc = null;

  function waitIceGathering(conn) {
    if (conn.iceGatheringState === 'complete') return Promise.resolve();
    return new Promise((resolve) => {
      const timer = setTimeout(resolve, 3000);
      conn.addEventListener('icegatheringstatechange', () => {
        if (conn.iceGatheringState === 'complete') { clearTimeout(timer); resolve(); }
      });
    });
  }

  return {
    async play(videoId, app) {
      this.stop();
      try {
        const conn = new RTCPeerConnection();
        pc = conn;
        conn.addTransceiver('video', { direction: 'recvonly' });
        conn.addTransceiver('audio', { direction: 'recvonly' });
        const stream = new MediaStream();
        conn.addEventListener('track', (e) => stream.addTrack(e.track));
        conn.addEventListener('connectionstatechange', () => app.send('state:' + conn.connectionState));
        const video = document.getElementById(videoId);
        video.srcObject = stream;

        await conn.setLocalDescription(await conn.createOffer());
        await waitIceGathering(conn);
        app.send('offer:' + conn.localDescription.sdp);
        const answer = await app.recv();
        if (answer.startsWith('error:')) { this.stop(); return; }
        await conn.setRemoteDescription({ type: 'answer', sdp: answer });
        // 자동 재생 정책 때문에 소리 없이 시작한다. 컨트롤에서 음소거를 풀 수 있다.
        video.muted = true;
        await video.play().catch(() => {});
      } catch (e) {
        app.send('error:' + (e.message || e));
      }
    },

    stop() {
      if (pc) { pc.close(); pc = null; }
    },
  };
})();
