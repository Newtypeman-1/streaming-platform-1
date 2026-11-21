// 토스페이먼츠 주문서형 결제(테스트 키를 쓰면 실제 돈이 빠져나가지 않는다).
// WASM(Rust) 쪽에서 document::eval 로 window.payment 의 함수를 부른다.
// 결제수단·약관 UI 를 주문서 안(#payment-method, #agreement)에 그리고, 결제하기를 누르면 고른 결제수단의 결제창을 연다.
window.payment = {
  widgets: null,
  agreed: false,

  // 서버가 만든 주문의 금액으로 결제 UI 를 그린다. 실패하면 { code, message }
  async render(clientKey, order) {
    if (typeof TossPayments === 'undefined') {
      return { code: 'SDK_NOT_LOADED', message: '결제 모듈을 불러오지 못했습니다.' };
    }
    try {
      const widgets = TossPayments(clientKey).widgets({ customerKey: TossPayments.ANONYMOUS });
      await widgets.setAmount({ currency: 'KRW', value: order.amount });
      const [, agreement] = await Promise.all([
        widgets.renderPaymentMethods({ selector: '#payment-method' }),
        widgets.renderAgreement({ selector: '#agreement' }),
      ]);
      this.agreed = true;
      agreement.on('agreementStatusChange', (status) => {
        this.agreed = status.agreedRequiredTerms;
      });
      this.widgets = widgets;
      return null;
    } catch (e) {
      return { code: e.code || 'UNKNOWN', message: e.message || String(e) };
    }
  },

  // 결제창을 연다. 인증에 성공하면 successUrl 로 이동하고, 서버가 승인을 확인한다.
  // 필수 약관에 동의하지 않았으면 { notice } (주문은 그대로 둔다).
  // 결제창을 닫거나 실패하면 { code, message } (환경에 따라 failUrl 로 이동하기도 한다).
  async request(order) {
    if (!this.widgets) {
      return { code: 'NOT_RENDERED', message: '결제 화면이 준비되지 않았습니다.' };
    }
    if (!this.agreed) {
      return { notice: '필수 약관에 동의해주세요.' };
    }
    const origin = location.origin;
    try {
      await this.widgets.requestPayment({
        orderId: order.orderId,
        orderName: order.orderName,
        successUrl: origin + '/donations/success',
        // 결제창을 닫으면 failUrl 에 orderId 가 붙지 않으므로 직접 넣어 둔다
        failUrl: origin + '/donations/fail?order=' + encodeURIComponent(order.orderId),
      });
      return null;
    } catch (e) {
      // PC 결제창을 닫으면 USER_CANCEL. 취소인지 실패인지는 서버가 코드를 보고 정한다
      return { code: e.code || 'UNKNOWN', message: e.message || String(e) };
    }
  },

  // 주문서를 닫을 때. 다음 주문은 새 금액으로 다시 그린다.
  clear() {
    this.widgets = null;
    this.agreed = false;
  },
};
