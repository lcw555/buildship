<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { NButton, NCheckbox, NInputNumber, NSwitch, useDialog, useMessage } from "naive-ui";
import EmptyState from "@/components/EmptyState.vue";
import { call, errorText } from "@/composables/useIpc";
import { formatBytes, formatTime } from "@/composables/useFormat";
import { useAppStore } from "@/stores/app";
import type { BackupEntry, BackupResult, CleanSummary, SecretReference } from "@/types/ipc";

const message = useMessage();
const dialog = useDialog();
const app = useAppStore();

const backups = ref<BackupEntry[]>([]);
const secrets = ref<SecretReference[]>([]);
const busy = ref(false);
const saving = ref(false);
/** 清理/卸载前是否把数据库备份导出到桌面 */
const keepBackup = ref(true);
const maintenanceBusy = ref(false);

const form = reactive({
  keepArtifacts: 20,
  startupMinimized: false,
  buildTimeoutMinutes: 30,
});

const node = computed(() => app.bootstrap?.toolchain.node ?? null);
const packageManagers = computed(() => app.bootstrap?.toolchain.packageManagers ?? []);

onMounted(async () => {
  form.keepArtifacts = Number(app.setting("artifact.keep", "20")) || 20;
  form.startupMinimized = app.setting("startup.minimized", "false") === "true";
  form.buildTimeoutMinutes = Number(app.setting("build.timeoutMinutes", "30")) || 30;
  await reload();
});

async function reload() {
  backups.value = await call<BackupEntry[]>("backup_list").catch(() => []);
  secrets.value = await call<SecretReference[]>("secret_list").catch(() => []);
}

async function refreshToolchain() {
  busy.value = true;
  try {
    await app.refreshToolchain();
    message.success("工具链检测已刷新");
  } catch (err) {
    message.error(errorText(err));
  } finally {
    busy.value = false;
  }
}

async function saveSettings() {
  saving.value = true;
  try {
    await app.updateSettings({
      "artifact.keep": String(form.keepArtifacts),
      "startup.minimized": String(form.startupMinimized),
      "build.timeoutMinutes": String(form.buildTimeoutMinutes),
    });
    message.success("设置已保存");
  } catch (err) {
    message.error(errorText(err));
  } finally {
    saving.value = false;
  }
}

async function createBackup() {
  busy.value = true;
  try {
    const result = await call<BackupResult>("backup_create");
    message.success(result.message);
    await reload();
  } catch (err) {
    message.error(errorText(err));
  } finally {
    busy.value = false;
  }
}

function restoreBackup(entry: BackupEntry) {
  dialog.warning({
    title: "确认恢复数据库",
    content: `将用备份 ${entry.fileName} 覆盖当前数据库。恢复前会自动为当前库创建一份快照，恢复后自动执行迁移与完整性检查。`,
    positiveText: "确认恢复",
    negativeText: "取消",
    onPositiveClick: async () => {
      busy.value = true;
      try {
        const result = await call<BackupResult>("backup_restore", { path: entry.path });
        message.success(result.message);
        await app.load();
        await reload();
      } catch (err) {
        message.error(errorText(err));
      } finally {
        busy.value = false;
      }
    },
  });
}

async function verifyBackup(entry: BackupEntry) {
  try {
    const actual = await call<string>("file_sha256", { path: entry.path });
    if (actual === entry.sha256) message.success("备份文件哈希一致");
    else message.error(`哈希不一致：记录 ${entry.sha256.slice(0, 12)}，实际 ${actual.slice(0, 12)}`);
  } catch (err) {
    message.error(errorText(err));
  }
}

async function openDataDir() {
  try {
    await call("open_data_dir");
  } catch (err) {
    message.error(errorText(err));
  }
}

/** 清理本机数据：删除后自动重启为全新状态 */
function cleanData() {
  const dir = app.bootstrap?.dataDir ?? "本机数据目录";
  dialog.error({
    title: "清理本机数据",
    content: `将删除 ${dir} 下的数据库、发布包、备份、构建日志与工作目录，操作不可撤销。${
      keepBackup.value ? "当前已勾选「先导出备份到桌面」。" : "当前未勾选备份，清理后无法找回。"
    }完成后应用会自动重启。`,
    positiveText: "清理并重启",
    negativeText: "取消",
    onPositiveClick: async () => {
      maintenanceBusy.value = true;
      try {
        const summary = await call<CleanSummary>("maintenance_clean", { backupFirst: keepBackup.value });
        message.success(summary.message);
      } catch (err) {
        message.error(errorText(err));
      } finally {
        maintenanceBusy.value = false;
      }
    },
  });
}

/** 一键卸载：清数据 + 调用安装包自带的卸载程序 */
function uninstallApp() {
  const dir = app.bootstrap?.dataDir ?? "本机数据目录";
  dialog.error({
    title: "一键卸载 Buildship",
    content: `将先清空 ${dir} 下的全部数据，再调用卸载程序移除应用，过程中应用会退出。${
      keepBackup.value ? "当前已勾选「先导出备份到桌面」。" : "当前未勾选备份，卸载后数据无法找回。"
    }`,
    positiveText: "卸载",
    negativeText: "取消",
    onPositiveClick: async () => {
      maintenanceBusy.value = true;
      try {
        const summary = await call<CleanSummary>("maintenance_uninstall", { backupFirst: keepBackup.value });
        if (summary.uninstaller) {
          message.info("卸载程序已启动，应用即将退出");
        } else {
          message.warning(summary.message);
        }
      } catch (err) {
        message.error(errorText(err));
      } finally {
        maintenanceBusy.value = false;
      }
    },
  });
}
</script>

<template>
  <div>
    <div class="card">
      <div class="card-head">
        <div>
          <div class="card-title">工具链</div>
          <div class="card-desc">构建依赖本机 Node.js 与包管理器；缺失时会在构建前拦截</div>
        </div>
        <div class="card-actions">
          <n-button size="small" :loading="busy" @click="refreshToolchain">重新检测</n-button>
        </div>
      </div>
      <div class="card-body">
        <table class="table">
          <thead>
            <tr>
              <th style="width: 140px">工具</th>
              <th style="width: 100px">状态</th>
              <th style="width: 120px">版本</th>
              <th>路径 / 说明</th>
            </tr>
          </thead>
          <tbody>
            <tr v-if="node">
              <td class="cell-main">Node.js</td>
              <td>
                <span class="badge" :class="node.available ? 'success' : 'danger'">
                  {{ node.available ? "可用" : "缺失" }}
                </span>
              </td>
              <td class="mono muted">{{ node.version ?? "-" }}</td>
              <td class="mono muted">{{ node.path ?? node.hint }}</td>
            </tr>
            <tr v-for="pm in packageManagers" :key="pm.name">
              <td class="cell-main">{{ pm.name }}</td>
              <td>
                <span class="badge" :class="pm.available ? 'success' : 'neutral'">
                  {{ pm.available ? "可用" : "未安装" }}
                </span>
              </td>
              <td class="mono muted">{{ pm.version ?? "-" }}</td>
              <td class="mono muted">{{ pm.path ?? pm.hint }}</td>
            </tr>
          </tbody>
        </table>
        <div class="field-hint" style="margin-top: 10px">
          检测时间：{{ formatTime(app.bootstrap?.toolchain.checkedAt) }} ·
          包管理器按项目 lockfile 自动识别，也可在项目配置中手动指定
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-head">
        <div>
          <div class="card-title">运行与构建</div>
          <div class="card-desc">这些设置保存在本地 SQLite 的 app_settings 表中</div>
        </div>
        <div class="card-actions">
          <n-button size="small" type="primary" :loading="saving" @click="saveSettings">保存</n-button>
        </div>
      </div>
      <div class="card-body">
        <div class="form-grid">
          <div>
            <label class="field-label">保留发布包数量</label>
            <n-input-number v-model:value="form.keepArtifacts" :min="1" :max="200" style="width: 100%" />
            <div class="field-hint">超出数量时提示清理，不会自动删除仍被发布记录引用的包</div>
          </div>
          <div>
            <label class="field-label">构建超时（分钟）</label>
            <n-input-number v-model:value="form.buildTimeoutMinutes" :min="5" :max="240" style="width: 100%" />
            <div class="field-hint">超时后自动终止整棵进程树并标记为失败</div>
          </div>
          <div>
            <label class="field-label">启动时最小化到托盘</label>
            <div style="padding-top: 6px">
              <n-switch v-model:value="form.startupMinimized" />
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-head">
        <div>
          <div class="card-title">数据与备份</div>
          <div class="card-desc">数据库位于系统应用数据目录，不写入安装目录或项目目录</div>
        </div>
        <div class="card-actions">
          <n-button size="small" @click="openDataDir">打开数据目录</n-button>
          <n-button size="small" type="primary" :loading="busy" @click="createBackup">立即备份</n-button>
        </div>
      </div>
      <div class="card-body">
        <dl class="kv">
          <dt>数据目录</dt>
          <dd class="mono">{{ app.bootstrap?.dataDir }}</dd>
          <dt>数据库文件</dt>
          <dd class="mono">{{ app.bootstrap?.dbPath }}</dd>
          <dt>Schema 版本</dt>
          <dd>v{{ app.bootstrap?.schemaVersion }}（WAL + 外键 + 审计触发器）</dd>
          <dt>当前用户</dt>
          <dd>{{ app.bootstrap?.operator }}</dd>
        </dl>

        <div class="card-title" style="margin: 18px 0 10px">备份文件</div>
        <table v-if="backups.length" class="table">
          <thead>
            <tr>
              <th>文件</th>
              <th style="width: 110px">大小</th>
              <th style="width: 180px">创建时间</th>
              <th style="width: 110px">SHA-256</th>
              <th style="width: 150px"></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="entry in backups" :key="entry.path">
              <td class="mono">{{ entry.fileName }}</td>
              <td class="muted">{{ formatBytes(entry.sizeBytes) }}</td>
              <td class="muted">{{ formatTime(entry.createdAt) }}</td>
              <td class="mono muted">{{ entry.sha256.slice(0, 10) }}</td>
              <td>
                <div class="row-actions">
                  <n-button size="tiny" quaternary @click="verifyBackup(entry)">校验</n-button>
                  <n-button size="tiny" quaternary type="warning" @click="restoreBackup(entry)">恢复</n-button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
        <EmptyState v-else title="还没有备份" desc="点击「立即备份」用 VACUUM INTO 生成一致性备份，并计算 SHA-256" />
      </div>
    </div>

    <div class="card">
      <div class="card-head">
        <div>
          <div class="card-title">系统凭据</div>
          <div class="card-desc">Agent Token 等敏感信息存放在系统凭据库，数据库仅保存引用</div>
        </div>
      </div>
      <div class="card-body">
        <table v-if="secrets.length" class="table">
          <thead>
            <tr>
              <th>名称</th>
              <th>存储位置</th>
              <th>最近使用</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="secret in secrets" :key="secret.id">
              <td class="cell-main">{{ secret.label }}</td>
              <td class="muted">
                {{ secret.provider === "windows_credential_manager" ? "Windows 凭据管理器" : "macOS 钥匙串" }}
              </td>
              <td class="muted">{{ secret.lastUsedAt ? formatTime(secret.lastUsedAt) : "从未" }}</td>
            </tr>
          </tbody>
        </table>
        <div v-else class="muted" style="font-size: 13px">
          还没有保存任何凭据。在「发布管理 → 添加站点」中填写 Agent Token 即可入库。
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-head">
        <div>
          <div class="card-title" style="color: var(--bs-danger)">重置与卸载</div>
          <div class="card-desc">清空本机数据，或一次性移除应用与数据</div>
        </div>
        <div class="card-actions">
          <n-checkbox v-model:checked="keepBackup">先导出备份到桌面</n-checkbox>
        </div>
      </div>
      <div class="card-body">
        <div class="danger-row">
          <div>
            <div class="cell-main">清理本机数据</div>
            <div class="cell-sub">
              删除数据库、发布包、备份、构建日志与工作目录，保留程序本身；完成后自动重启为全新状态。
            </div>
          </div>
          <n-button size="small" type="warning" secondary :loading="maintenanceBusy" @click="cleanData">
            清理数据
          </n-button>
        </div>
        <div class="danger-row">
          <div>
            <div class="cell-main">一键卸载</div>
            <div class="cell-sub">
              先清空本机数据，再调用安装包自带的卸载程序移除应用并退出；便携或开发运行时会只清理数据。
            </div>
          </div>
          <n-button size="small" type="error" :loading="maintenanceBusy" @click="uninstallApp">
            卸载 Buildship
          </n-button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.danger-row {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 14px 0;
  border-bottom: 1px solid var(--bs-border);
}

.danger-row:first-child {
  padding-top: 0;
}

.danger-row:last-child {
  padding-bottom: 0;
  border-bottom: none;
}

.danger-row .cell-main {
  font-size: 13.5px;
  font-weight: 600;
}

.danger-row .cell-sub {
  margin-top: 4px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--bs-muted);
}

.danger-row > :last-child {
  flex: none;
}
</style>
