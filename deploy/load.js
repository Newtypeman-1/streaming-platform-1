// 부하 발생 스크립트(k6). deploy/load.sh 가 Docker 로 실행한다.
// 시청자가 방송 목록을 보고, 방송 하나를 골라 상세·댓글을 읽는 흐름을 반복한다.
import http from 'k6/http';
import { check } from 'k6';

const BASE = __ENV.BASE_URL || 'http://nadri-control-plane:30800';

export const options = {
  vus: Number(__ENV.VUS || 50),
  duration: __ENV.DURATION || '3m',
  // 요청마다 새 연결 → 접속자가 많은 상황처럼 Service 가 요청을 여러 인스턴스로 고르게 나눈다
  noConnectionReuse: true,
  thresholds: { http_req_failed: ['rate<0.01'] },
};

export default function () {
  const list = http.get(`${BASE}/api/broadcasts`, { tags: { name: 'list' } });
  check(list, { 'list 200': (r) => r.status === 200 });
  const broadcasts = list.status === 200 ? list.json() : [];
  if (broadcasts.length === 0) return;
  const id = broadcasts[Math.floor(Math.random() * broadcasts.length)].id;
  check(http.get(`${BASE}/api/broadcasts/${id}`, { tags: { name: 'detail' } }), { 'detail 200': (r) => r.status === 200 });
  check(http.get(`${BASE}/api/broadcasts/${id}/comments`, { tags: { name: 'comments' } }), { 'comments 200': (r) => r.status === 200 });
}
