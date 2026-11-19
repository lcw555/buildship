#!/usr/bin/env bash
# Buildship Agent 端到端冒烟脚本
#
# 覆盖 docs/testing.md 第 7.3 节声明的全部场景，任何机器上都能一键复现：
#   bash scripts/e2e-agent-smoke.sh
#
# 依赖：curl、python（仅用于生成测试 ZIP）、已构建的 target/debug/deploy-agent。
# 说明：脚本自己起 Agent、自己收尾，不污染已有环境；日志与站点落在临时目录。

set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# 每次运行用独立临时目录：避免上一轮的版本目录与幂等缓存影响本次断言
WORK="$(mktemp -d "${TMPDIR:-/tmp}/buildship-e2e.XXXXXX")"
AGENT="$ROOT/target/debug/deploy-agent"
TOKEN="e2e-token-0123456789abcdef"
BASE="http://127.0.0.1:7899"
SITE="$WORK/site"
PORT="${BASE##*:}"
echo "工作目录：$WORK"

pass=0
fail=0

check() {
  local label="$1" expected="$2" actual="$3"
  if [[ "$actual" == *"$expected"* ]]; then
    printf '  \033[32m✓\033[0m %s\n' "$label"
    pass=$((pass + 1))
  else
    printf '  \033[31m✗\033[0m %s\n     期望包含: %s\n     实际: %s\n' "$label" "$expected" "$actual"
    fail=$((fail + 1))
  fi
}

if [[ ! -x "$AGENT.exe" && ! -x "$AGENT" ]]; then
  echo "未找到 $AGENT，请先执行：cargo build -p deploy-agent" >&2
  exit 1
fi
BIN="$AGENT"
[[ -x "$AGENT.exe" ]] && BIN="$AGENT.exe"

mkdir -p "$WORK"

cat > "$WORK/agent.toml" <<EOF
bind = "127.0.0.1:$PORT"
token = "$TOKEN"
maxUploadMb = 64
healthTimeoutSecs = 5
linkStrategy = "auto"

[[sites]]
name = "demo"
physicalPath = "$(cygpath -m "$SITE" 2>/dev/null || echo "$SITE")"
keepReleases = 3
EOF

make_package() {
  # $1 = 输出 zip 路径，$2 = index.html 内容
  local zip_path="$1" body="$2"
  local src="$WORK/src"
  rm -rf "$src"
  mkdir -p "$src/assets"
  printf '%s' "$body" > "$src/index.html"
  printf 'console.log("build")' > "$src/assets/app.js"
  python - "$src" "$zip_path" <<'PY'
import hashlib, json, os, sys, zipfile
src, out = sys.argv[1], sys.argv[2]
files = []
for dirpath, _, names in os.walk(src):
    for name in names:
        full = os.path.join(dirpath, name)
        rel = os.path.relpath(full, src).replace("\\", "/")
        files.append((rel, full))
files.sort()
manifest = {
    "schemaVersion": 1,
    "files": [
        {
            "path": rel,
            "size": os.path.getsize(full),
            "sha256": hashlib.sha256(open(full, "rb").read()).hexdigest(),
        }
        for rel, full in files
    ],
}
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("manifest.json", json.dumps(manifest).encode())
    for rel, full in files:
        z.write(full, rel)
print(hashlib.sha256(open(out, "rb").read()).hexdigest())
PY
}

upload() {
  # $1 = releaseId，$2 = zip 路径，$3 = 声明的 sha256，$4 = 站点名
  curl -s -m 60 -o /dev/null -w '%{http_code}' -X POST "$BASE/v1/upload" \
    -H "x-buildship-token: $TOKEN" \
    -H "x-buildship-release-id: $1" \
    -H "x-buildship-sha256: $3" \
    -H "x-buildship-site: ${4:-demo}" \
    -H "Content-Type: application/zip" \
    --data-binary "@$2"
}

upload_body() {
  curl -s -m 60 -X POST "$BASE/v1/upload" \
    -H "x-buildship-token: $TOKEN" \
    -H "x-buildship-release-id: $1" \
    -H "x-buildship-sha256: $3" \
    -H "x-buildship-site: ${4:-demo}" \
    -H "Content-Type: application/zip" \
    --data-binary "@$2"
}

apply() {
  # $1 = releaseId，$2 = requestId，$3 = healthCheckUrl（可空）
  local health=""
  [[ -n "${3:-}" ]] && health=$(printf ',"healthCheckUrl":"%s"' "$3")
  curl -s -m 60 -X POST "$BASE/v1/apply" \
    -H "x-buildship-token: $TOKEN" -H "Content-Type: application/json" \
    -d "{\"releaseId\":\"$1\",\"site\":\"demo\",\"physicalPath\":\"$(cygpath -m "$SITE" 2>/dev/null || echo "$SITE")\"$health,\"requestId\":\"$2\"}"
}

echo "==> 启动 Agent（$BASE）"
"$BIN" --config "$WORK/agent.toml" > "$WORK/agent.log" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null' EXIT
for _ in $(seq 1 30); do
  curl -s -m 1 "$BASE/healthz" > /dev/null 2>&1 && break
  sleep 0.2
done

echo "==> 1. 探活与鉴权"
check "GET /healthz 返回 ok" "ok" "$(curl -s -m 5 "$BASE/healthz")"
check "无 token 被拒（403）" "403" "$(curl -s -m 5 -o /dev/null -w '%{http_code}' "$BASE/v1/status")"
check "错误 token 被拒（403）" "CREDENTIAL_UNAVAILABLE" \
  "$(curl -s -m 5 -H 'x-buildship-token: wrong' "$BASE/v1/status")"
check "正确 token 返回站点信息" '"site":"demo"' \
  "$(curl -s -m 5 -H "x-buildship-token: $TOKEN" "$BASE/v1/status")"

echo "==> 2. 上传与哈希校验"
SHA1=$(make_package "$WORK/v1.zip" '<html>v1</html>')
body=$(upload_body rel-v1 "$WORK/v1.zip" "$SHA1")
check "上传成功且非复用" '"alreadyExisted":false' "$body"
check "返回目录树哈希" '"treeSha256"' "$body"
check "manifest 未落进站点目录" "0" "$(ls "$SITE/releases/rel-v1/manifest.json" 2>/dev/null | wc -l | tr -d ' ')"
SHA_BAD=$(printf '%064d' 0)
check "哈希不符被拒（422）" "422" "$(upload rel-bad "$WORK/v1.zip" "$SHA_BAD")"

echo "==> 3. 发布 v1（健康检查通过）"
resp=$(apply rel-v1 req-1 "$BASE/healthz")
check "应用成功" '"rolledBack":false' "$resp"
check "健康检查通过" '"healthStatus":"passed"' "$resp"
check "使用目录联接切换" '"switchMode":"link"' "$(grep -o '"switchMode":"[a-z]*"' "$SITE/.buildship/audit.jsonl" | head -1)"
check "current 指向 v1 内容" "<html>v1</html>" "$(cat "$SITE/current/index.html")"

echo "==> 4. 幂等重放"
first=$(apply rel-v1 req-1 "$BASE/healthz")
check "同 requestId 返回相同 appliedAt" "$(echo "$resp" | grep -o '"appliedAt":"[^"]*"')" "$(echo "$first" | grep -o '"appliedAt":"[^"]*"')"

echo "==> 5. 发布 v2 并记录上一版本"
SHA2=$(make_package "$WORK/v2.zip" '<html>v2</html>')
upload rel-v2 "$WORK/v2.zip" "$SHA2" > /dev/null
resp2=$(apply rel-v2 req-2 "$BASE/healthz")
check "v2 应用成功" '"releaseId":"rel-v2"' "$resp2"
check "记录上一版本为 rel-v1" '"previousReleaseId":"rel-v1"' "$resp2"
check "current 已更新为 v2" "<html>v2</html>" "$(cat "$SITE/current/index.html")"

echo "==> 6. 回滚到 v1"
rb=$(curl -s -m 60 -X POST "$BASE/v1/rollback" \
  -H "x-buildship-token: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"toReleaseId\":\"rel-v1\",\"site\":\"demo\",\"physicalPath\":\"$(cygpath -m "$SITE" 2>/dev/null || echo "$SITE")\",\"reason\":\"冒烟测试回滚\",\"requestId\":\"req-3\"}")
check "回滚成功" '"releaseId":"rel-v1"' "$rb"
check "current 回到 v1 内容" "<html>v1</html>" "$(cat "$SITE/current/index.html")"
check "回滚原因写入审计" "1" "$(grep -c '冒烟测试回滚' "$SITE/.buildship/audit.jsonl" || echo 0)"

echo "==> 7. 健康检查失败 → 自动回滚"
SHA3=$(make_package "$WORK/v3.zip" '<html>v3-bad</html>')
upload rel-v3 "$WORK/v3.zip" "$SHA3" > /dev/null
bad=$(apply rel-v3 req-4 "$BASE/no-such-health-endpoint")
check "健康检查失败" '"healthStatus":"failed"' "$bad"
check "已自动回滚" '"rolledBack":true' "$bad"
check "站点内容回到 v1" "<html>v1</html>" "$(cat "$SITE/current/index.html")"

echo "==> 8. 安全边界"
check "越权 physicalPath 被拒（403）" "PERMISSION_DENIED" \
  "$(curl -s -m 10 -X POST "$BASE/v1/apply" \
     -H "x-buildship-token: $TOKEN" -H "Content-Type: application/json" \
     -d '{"releaseId":"rel-v1","site":"demo","physicalPath":"C:/Windows/System32","requestId":"req-5"}')"
check "不存在的版本（404）" "404" \
  "$(curl -s -m 10 -o /dev/null -w '%{http_code}' -X POST "$BASE/v1/apply" \
     -H "x-buildship-token: $TOKEN" -H "Content-Type: application/json" \
     -d "{\"releaseId\":\"rel-missing\",\"site\":\"demo\",\"physicalPath\":\"$(cygpath -m "$SITE" 2>/dev/null || echo "$SITE")\",\"requestId\":\"req-6\"}")"

echo "==> 9. 磁盘布局"
for path in releases/rel-v1 current .buildship/state.json .buildship/audit.jsonl .buildship/idempotency; do
  if [[ -e "$SITE/$path" ]]; then
    check "存在 $path" "" ""
  else
    printf '  \033[31m✗\033[0m 缺失 %s\n' "$path"
    fail=$((fail + 1))
  fi
done

echo
echo "==== 结果：$pass 通过 / $fail 失败 ===="
[[ $fail -eq 0 ]] || { echo "Agent 日志尾部："; tail -20 "$WORK/agent.log"; }
exit $((fail > 0))
