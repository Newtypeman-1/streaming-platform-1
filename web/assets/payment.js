// 토스페이먼츠 결제창(테스트 키를 쓰면 실제 돈이 빠져나가지 않는다).
// WASM(Rust) 쪽에서 document::eval 로 window.payment.request 를 부른다.
window.payment = {
  // 결제창 인증에 성공하면 successUrl 로 이동하고, 서버가 승인을 확인한다.
  // 결제창을 닫거나 실패하면 { code, message } 를 돌려준다 (환경에 따라 failUrl 로 이동하기도 한다).
  async request(clientKey, order) {
    if (typeof TossPayments === 'undefined') {
      return { code: 'SDK_NOT_LOADED', message: '결제 모듈을 불러오지 못했습니다.' };
    }
    const origin = location.origin;
    try {
      const payment = TossPayments(clientKey).payment({ customerKey: TossPayments.ANONYMOUS });
      await payment.requestPayment({
        method: 'CARD',
        amount: { currency: 'KRW', value: order.amount },
        orderId: order.orderId,
        orderName: order.orderName,
        successUrl: origin + '/donations/success',
        // 결제창을 닫으면 failUrl 에 orderId 가 붙지 않으므로 직접 넣어 둔다
        failUrl: origin + '/donations/fail?order=' + encodeURIComponent(order.orderId),
      });
      return null;
    } catch (e) {
      // PC 결제창을 닫으면 USER_CANCEL → 서버에는 결제창 취소 코드로 알린다
      const code = e.code === 'USER_CANCEL' ? 'PAY_PROCESS_CANCELED' : (e.code || 'UNKNOWN');
      return { code, message: e.message || String(e) };
    }
  },
};
