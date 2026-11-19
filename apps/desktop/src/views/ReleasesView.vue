<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { NButton, NDrawer, NDrawerContent, NInput, NModal, NSelect, NSwitch, useMessage } from "naive-ui";
import StatusPill from "@/components/StatusPill.vue";
import EmptyState from "@/components/EmptyState.vue";
import { call, errorText } from "@/composables/useIpc";
import {
  ENV_KIND_TEXT,
  formatBytes,
  formatRelative,
  formatTime,
  shortSha,
  statusText,
} from "@/composables/useFormat";
import { useReleasesStore } from "@/stores/releases";
import { useProjectsStore } from "@/stores/projects";
import type { EnvironmentKind, Release, ReleaseDetail, SecretReference } from "@/types/ipc";

const message = useMessage();
const releases = useReleasesStore();
const projects = useProjectsStore();

const envOpen = ref(false);
const targetOpen = ref(false);
const rollbackOpen = ref(false);
const detailOpen = ref(false);
const detail = ref<ReleaseDetail | null>(null);
const saving = ref(false);

const envForm = reactive({
  id: null as string | null,
  name: "",
  kind: "test" as EnvironmentKind,
  description: "",
});

const targetForm = reactive({
  id: null as string | null,
  environmentId: "",
  name: "",
  serverHost: "",
  agentBaseUrl: "http://127.0.0.1:18631",
  iisSite: "",
  physicalPath: "D:\\\\sites\\\\my-app",
  healthCheckUrl: "",
  tokenLabel: "",
  token: "",
  tlsVerify: true,
});

const rollbackForm = reactive({
  targetId: null as string | null,
  reason: "",
  candidates: [] as Release[],
});

const secrets = ref<SecretReference[]>([]);

const kindOptions = [
  { label: "开发", value: "dev" },
  { label: "测试", value: "test" },
  { label: "预发布", value: "staging" },
  { label: "生产", value: "prod" },
];

const projectFilter = ref<string | null>(null);
const projectOptions = computed(() => [
  { label: "全部项目", value: null as unknown as string },
  ...projects.items.map((item) => ({ label: item.name, value: item.id })),
]);

const current = computed(() => releases.releases.find((item) => item.isCurrent) ?? null);

onMounted(async () => {
  await projects.load();
  await releases.loadEnvironments();
  await releases.load(projectFilter.value ?? undefined);
  secrets.value = await call<SecretReference[]>("secret_list").catch(() => []);
});

async function reload() {
  await releases.load(projectFilter.value ?? undefined);
  await releases.loadEnvironments();
}

function openEnv(environment?: { id: string; name: string; kind: EnvironmentKind; description: string | null }) {
  envForm.id = environment?.id ?? null;
  envForm.name = environment?.name ?? "";
  envForm.kind = environment?.kind ?? "test";
  envForm.description = environment?.description ?? "";
  envOpen.value = true;
}

async function saveEnv() {
  saving.value = true;
  try {
    await releases.saveEnvironment({
      id: envForm.id,
      name: envForm.name,
      kind: envForm.kind,
      description: envForm.description || null,
    });
    message.success("环境已保存");
    envOpen.value = false;
  } catch (err) {
    message.error(errorText(err));
  } finally {
    saving.value = false;
  }
}

function openTarget(environmentId: string) {
  targetForm.id = null;
  targetForm.environmentId = environmentId;
  targetForm.name = "";
  targetForm.serverHost = "";
  targetForm.iisSite = "";
  targetForm.healthCheckUrl = "";
  targetForm.tokenLabel = "";
  targetForm.token = "";
  targetOpen.value = true;
}

async function saveTarget() {
  saving.value = true;
  try {
    let credentialRef: string | null = null;
    if (targetForm.token.trim()) {
      const label = targetForm.tokenLabel.trim() || `${targetForm.name || "agent"}-token`;
      const reference = await call<SecretReference>("secret_save", {
        label,
        token: targetForm.token.trim(),
      });
      credentialRef = reference.id;
    }
    await releases.saveTarget({
      id: targetForm.id,
      environmentId: targetForm.environmentId,
      name: targetForm.name,
      serverHost: targetForm.serverHost || null,
      agentBaseUrl: targetForm.agentBaseUrl,
      iisSite: targetForm.iisSite,
      physicalPath: targetForm.physicalPath,
      healthCheckUrl: targetForm.healthCheckUrl || null,
      credentialRef,
      tlsVerify: targetForm.tlsVerify,
    });
    message.success("部署目标已保存");
    targetOpen.value = false;
  } catch (err) {
    message.error(errorText(err));
  } finally {
    saving.value = false;
  }
}

async function checkTarget(id: string) {
  try {
    const result = await call<{ ok: boolean; message: string }>("target_check", { id });
    if (result.ok) message.success(result.message);
    else message.error(result.message);
    await releases.loadEnvironments();
  } catch (err) {
    message.error(errorText(err));
  }
}

async function openRollback(release: Release) {
  rollbackForm.reason = "";
  try {
    const candidates = await releases.healthyCandidates(release.projectId, release.environmentId);
    rollbackForm.candidates = candidates.filter((item) => !item.isCurrent);
    rollbackForm.targetId = rollbackForm.candidates[0]?.id ?? null;
    rollbackOpen.value = true;
  } catch (err) {
    message.error(errorText(err));
  }
}

async function submitRollback() {
  if (!rollbackForm.targetId) {
    message.warning("请选择要回滚到的版本");
    return;
  }
  if (rollbackForm.reason.trim().length < 2) {
    message.warning("请填写回滚原因");
    return;
  }
  saving.value = true;
  try {
    await releases.rollback(rollbackForm.targetId, rollbackForm.reason.trim());
    message.success("回滚任务已创建");
    rollbackOpen.value = false;
    await reload();
  } catch (err) {
    message.error(errorText(err));
  } finally {
    saving.value = false;
  }
}

async function openDetail(release: Release) {
  try {
    detail.value = await releases.loadDetail(release.id);
    detailOpen.value = true;
  } catch (err) {
    message.error(errorText(err));
  }
}

function candidateLabel(release: Release): string {
  const env = releases.environments.find((item) => item.environment.id === release.environmentId)
    ?.environment.name;
  return `${release.version} · ${env ?? ""} · ${formatTime(release.createdAt)}`;
}
</script>

<template>
  <div>
    <div class="toolbar">
      <n-button type="primary" @click="openEnv()">新建环境</n-button>
      <n-button quaternary @click="reload">刷新</n-button>
      <div class="spacer" />
      <span v-if="current" class="chip">
        当前线上版本 · {{ current.version }}
      </span>
    </div>

    <div v-if="releases.environments.length" class="stat-grid" style="margin-bottom: 16px">
      <div v-for="group in releases.environments" :key="group.environment.id" class="card">
        <div class="card-head">
          <div>
            <div class="card-title">
              {{ group.environment.name }}
              <span class="badge plain info" style="margin-left: 6px">
                {{ ENV_KIND_TEXT[group.environment.kind] }}
              </span>
            </div>
            <div class="card-desc">{{ group.environment.description || "未填写说明" }}</div>
          </div>
          <div class="card-actions">
            <n-button size="tiny" @click="openTarget(group.environment.id)">添加站点</n-button>
            <n-button
              size="tiny"
              quaternary
              @click="openEnv(group.environment)"
            >
              编辑
            </n-button>
          </div>
        </div>
        <div class="card-body">
          <div v-if="!group.targets.length" class="muted" style="font-size: 12.5px">
            尚未配置 IIS 站点，点击「添加站点」填写 Agent 地址与物理目录
          </div>
          <div v-for="target in group.targets" :key="target.id" style="padding: 6px 0; border-bottom: 1px dashed var(--bs-border)">
            <div style="display: flex; align-items: center; gap: 8px">
              <div style="min-width: 0">
                <div class="cell-main">{{ target.name }}</div>
                <div class="cell-sub mono">{{ target.iisSite }} · {{ target.agentBaseUrl }}</div>
              </div>
              <div class="spacer" />
              <StatusPill
                v-if="target.lastCheckOk !== null"
                :status="target.lastCheckOk ? 'succeeded' : 'failed'"
                :label="target.lastCheckOk ? '在线' : '离线'"
              />
              <n-button size="tiny" quaternary @click="checkTarget(target.id)">检测</n-button>
            </div>
            <div class="cell-sub mono" style="margin-top: 4px">{{ target.physicalPath }}</div>
          </div>
        </div>
      </div>
    </div>
    <div v-else class="card">
      <EmptyState
        title="还没有发布环境"
        desc="先创建「测试 / 预发布 / 生产」环境，再为每个环境添加 Windows Server 上的 IIS 站点与 Deploy Agent 地址"
      >
        <n-button size="small" type="primary" @click="openEnv()">新建环境</n-button>
      </EmptyState>
    </div>

    <div class="card" style="margin-top: 16px">
      <div class="card-head">
        <div>
          <div class="card-title">发布记录</div>
          <div class="card-desc">每次发布生成独立版本，历史包不覆盖</div>
        </div>
        <div class="card-actions">
          <n-select
            v-model:value="projectFilter"
            :options="projectOptions"
            style="width: 180px"
            size="small"
            @update:value="reload"
          />
        </div>
      </div>
      <div class="card-body tight">
        <table v-if="releases.releases.length" class="table">
          <thead>
            <tr>
              <th>版本</th>
              <th>状态</th>
              <th>健康检查</th>
              <th>发布人</th>
              <th>时间</th>
              <th style="text-align: right">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="release in releases.releases" :key="release.id">
              <td>
                <div class="cell-main mono">
                  {{ release.version }}
                  <span v-if="release.isCurrent" class="badge success plain" style="margin-left: 6px">当前</span>
                </div>
                <div class="cell-sub">
                  {{ releases.environments.find((g) => g.environment.id === release.environmentId)?.environment.name ?? "" }}
                  · {{ release.id.slice(0, 8) }}
                </div>
              </td>
              <td><StatusPill :status="release.status" /></td>
              <td class="muted">{{ statusText(release.healthStatus) }}</td>
              <td class="muted">{{ release.operator ?? "-" }}</td>
              <td class="muted">{{ formatRelative(release.createdAt) }}</td>
              <td>
                <div class="row-actions">
                  <n-button size="tiny" @click="openDetail(release)">详情</n-button>
                  <n-button
                    size="tiny"
                    quaternary
                    :disabled="release.isCurrent || release.status !== 'healthy'"
                    @click="openRollback(release)"
                  >
                    回滚到此版本
                  </n-button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
        <EmptyState
          v-else
          title="还没有发布记录"
          desc="在「发布包」页选择发布包并指定目标环境即可发起发布"
        />
      </div>
    </div>

    <!-- 环境表单 -->
    <n-modal v-model:show="envOpen" preset="card" :title="envForm.id ? '编辑环境' : '新建环境'" style="width: 520px" :bordered="false">
      <div class="form-grid">
        <div>
          <label class="field-label">环境名称</label>
          <n-input v-model:value="envForm.name" placeholder="测试环境" />
        </div>
        <div>
          <label class="field-label">类型</label>
          <n-select v-model:value="envForm.kind" :options="kindOptions" />
        </div>
        <div class="full">
          <label class="field-label">说明</label>
          <n-input v-model:value="envForm.description" type="textarea" :autosize="{ minRows: 2, maxRows: 4 }" placeholder="例如：内部测试用 IIS 站点" />
        </div>
      </div>
      <template #footer>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button @click="envOpen = false">取消</n-button>
          <n-button type="primary" :loading="saving" @click="saveEnv">保存</n-button>
        </div>
      </template>
    </n-modal>

    <!-- 部署目标表单 -->
    <n-modal v-model:show="targetOpen" preset="card" title="IIS 站点与 Agent" style="width: 660px" :bordered="false">
      <div class="form-grid">
        <div>
          <label class="field-label">站点名称</label>
          <n-input v-model:value="targetForm.name" placeholder="主站" />
        </div>
        <div>
          <label class="field-label">服务器地址</label>
          <n-input v-model:value="targetForm.serverHost" placeholder="10.0.0.12" />
        </div>
        <div>
          <label class="field-label">Agent 地址</label>
          <n-input v-model:value="targetForm.agentBaseUrl" placeholder="http://10.0.0.12:18631" />
        </div>
        <div>
          <label class="field-label">IIS 站点标识</label>
          <n-input v-model:value="targetForm.iisSite" placeholder="my-app" />
        </div>
        <div class="full">
          <label class="field-label">站点物理目录</label>
          <n-input v-model:value="targetForm.physicalPath" placeholder="D:\\sites\\my-app" />
          <div class="field-hint">
            Agent 会在该目录下维护 releases / current / staging / agent 结构，并做目录边界检查
          </div>
        </div>
        <div class="full">
          <label class="field-label">健康检查 URL（可选）</label>
          <n-input v-model:value="targetForm.healthCheckUrl" placeholder="http://10.0.0.12/health.txt" />
        </div>
        <div>
          <label class="field-label">凭据名称</label>
          <n-input v-model:value="targetForm.tokenLabel" placeholder="prod-agent-token" />
        </div>
        <div>
          <label class="field-label">Agent Token</label>
          <n-input v-model:value="targetForm.token" type="password" placeholder="与 Agent 配置一致" />
          <div class="field-hint">Token 存入系统凭据库（Windows Credential Manager / macOS Keychain），不写数据库</div>
        </div>
        <div class="full">
          <label class="field-label">校验 TLS 证书</label>
          <n-switch v-model:value="targetForm.tlsVerify" />
          <span class="muted" style="margin-left: 10px; font-size: 12px">
            自签名证书的内网环境可关闭，生产环境建议开启
          </span>
        </div>
      </div>
      <template #footer>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button @click="targetOpen = false">取消</n-button>
          <n-button type="primary" :loading="saving" @click="saveTarget">保存</n-button>
        </div>
      </template>
    </n-modal>

    <!-- 回滚确认 -->
    <n-modal v-model:show="rollbackOpen" preset="card" title="确认回滚" style="width: 560px" :bordered="false">
      <div class="form-grid">
        <div class="full">
          <label class="field-label">回滚目标</label>
          <n-select
            v-model:value="rollbackForm.targetId"
            :options="rollbackForm.candidates.map((item) => ({ label: candidateLabel(item), value: item.id }))"
            placeholder="选择历史健康版本"
          />
          <div v-if="!rollbackForm.candidates.length" class="field-hint">该环境暂无可回滚的历史健康版本</div>
        </div>
        <div class="full">
          <label class="field-label">回滚原因（会写入审计）</label>
          <n-input v-model:value="rollbackForm.reason" type="textarea" :autosize="{ minRows: 2, maxRows: 4 }" placeholder="例如：新版首页白屏，回退到上一稳定版本" />
        </div>
      </div>
      <div class="field-hint" style="margin-top: 12px">
        回滚会重新执行健康检查；失败时保留错误现场且不会删除任何发布包。
      </div>
      <template #footer>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button @click="rollbackOpen = false">取消</n-button>
          <n-button type="primary" :loading="saving" @click="submitRollback">确认回滚</n-button>
        </div>
      </template>
    </n-modal>

    <!-- 发布详情 -->
    <n-drawer v-model:show="detailOpen" :width="520" placement="right">
      <n-drawer-content :title="`发布详情 · ${detail?.release.version ?? ''}`" closable>
        <div v-if="detail">
          <dl class="kv">
            <dt>状态</dt>
            <dd><StatusPill :status="detail.release.status" /></dd>
            <dt>健康检查</dt>
            <dd>{{ statusText(detail.release.healthStatus) }} {{ detail.release.healthMessage ?? "" }}</dd>
            <dt>环境</dt>
            <dd>{{ detail.environment?.name ?? "-" }}</dd>
            <dt>IIS 站点</dt>
            <dd class="mono">{{ detail.target?.iisSite ?? "-" }}</dd>
            <dt>发布包</dt>
            <dd class="mono">{{ detail.artifact?.fileName ?? "-" }}</dd>
            <dt>SHA-256</dt>
            <dd class="mono">{{ shortSha(detail.artifact?.sha256) }}</dd>
            <dt>大小</dt>
            <dd>{{ formatBytes(detail.artifact?.sizeBytes) }}</dd>
            <dt>发布人</dt>
            <dd>{{ detail.release.operator ?? "-" }}</dd>
            <dt>开始时间</dt>
            <dd>{{ formatTime(detail.release.startedAt) }}</dd>
            <dt>完成时间</dt>
            <dd>{{ formatTime(detail.release.finishedAt) }}</dd>
            <dt>回滚信息</dt>
            <dd>{{ detail.release.rollbackReason ?? "无" }}</dd>
          </dl>

          <div class="card-title" style="margin: 20px 0 12px">过程时间线</div>
          <div class="timeline">
            <div v-for="event in detail.events" :key="event.id" class="timeline-item">
              <div class="timeline-dot" :class="event.level" />
              <div class="timeline-body">
                <div>{{ event.message }}</div>
                <div class="timeline-time">{{ event.phase }} · {{ formatTime(event.createdAt) }}</div>
              </div>
            </div>
            <div v-if="!detail.events.length" class="muted">暂无事件</div>
          </div>
        </div>
        <div v-else class="muted">加载中…</div>
      </n-drawer-content>
    </n-drawer>
  </div>
</template>
