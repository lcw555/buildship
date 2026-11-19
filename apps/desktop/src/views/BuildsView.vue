<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NButton, NSelect, useMessage } from "naive-ui";
import StatusPill from "@/components/StatusPill.vue";
import EmptyState from "@/components/EmptyState.vue";
import { errorText } from "@/composables/useIpc";
import { ansiSpans, clearAnsiCache } from "@/composables/useAnsi";
import { formatBytes, formatDuration, formatTime, shortSha } from "@/composables/useFormat";
import { useBuildsStore } from "@/stores/builds";
import { useProjectsStore } from "@/stores/projects";
import { useArtifactsStore } from "@/stores/artifacts";
import { useAppStore } from "@/stores/app";
import type { BuildTask } from "@/types/ipc";

const route = useRoute();
const router = useRouter();
const message = useMessage();
const builds = useBuildsStore();
const projects = useProjectsStore();
const artifacts = useArtifactsStore();
const app = useAppStore();

/** 一次最多渲染的日志行数：上万行全量渲染会让界面明显卡顿 */
const MAX_RENDER = 1200;

const selectedId = ref<string | null>(null);
const consoleEl = ref<HTMLElement | null>(null);
const autoScroll = ref(true);
const artifactFor = ref<BuildTask | null>(null);
const creating = ref(false);

const selected = computed(() => builds.tasks.find((task) => task.id === selectedId.value) ?? null);
const lines = computed(() => (selectedId.value ? builds.logLines[selectedId.value] ?? [] : []));
const visibleLines = computed(() =>
  lines.value.length > MAX_RENDER ? lines.value.slice(-MAX_RENDER) : lines.value,
);
const hiddenCount = computed(() => Math.max(0, lines.value.length - MAX_RENDER));
const projectOptions = computed(() =>
  projects.items.map((item) => ({ label: item.name, value: item.id })),
);

onMounted(async () => {
  await projects.load();
  await builds.load();
  const taskParam = route.query.task as string | undefined;
  if (taskParam) {
    selectedId.value = taskParam;
  } else if (builds.tasks.length > 0) {
    selectedId.value = builds.tasks[0].id;
  }
  if (selectedId.value) {
    await loadLogs(selectedId.value);
    scrollToBottom();
  }
});

onUnmounted(() => {
  artifactFor.value = null;
  clearAnsiCache();
});

watch(selectedId, async (id) => {
  if (!id) return;
  await loadLogs(id);
  scrollToBottom();
});

watch(
  () => lines.value.length,
  () => {
    if (autoScroll.value) scrollToBottom();
  },
);

async function loadLogs(taskId: string) {
  try {
    const hasCache = (builds.logLines[taskId] ?? []).length > 0;
    if (!hasCache) {
      await builds.loadLogs(taskId, true);
    }
  } catch (err) {
    message.error(errorText(err));
  }
}

function scrollToBottom() {
  void nextTick(() => {
    if (consoleEl.value) {
      consoleEl.value.scrollTop = consoleEl.value.scrollHeight;
    }
  });
}

function onConsoleScroll(event: Event) {
  const el = event.target as HTMLElement;
  autoScroll.value = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
}

function toggleFollow() {
  autoScroll.value = !autoScroll.value;
  if (autoScroll.value) scrollToBottom();
}

async function startBuild(projectId: string | null) {
  if (!projectId) {
    message.warning("请选择项目");
    return;
  }
  try {
    const task = await builds.start(projectId);
    selectedId.value = task.id;
    builds.logLines[task.id] = [];
    message.success("构建已启动");
    app.loadQuick();
  } catch (err) {
    message.error(errorText(err));
  }
}

async function cancelBuild() {
  if (!selected.value) return;
  try {
    await builds.cancel(selected.value.id);
    message.info("已发送取消指令");
  } catch (err) {
    message.error(errorText(err));
  }
}

async function createArtifact(task: BuildTask) {
  creating.value = true;
  artifactFor.value = task;
  try {
    const artifact = await artifacts.create(task.id);
    message.success(`发布包已生成：${artifact.fileName}`);
    router.push("/artifacts");
  } catch (err) {
    message.error(errorText(err));
  } finally {
    creating.value = false;
  }
}

function statusSummary(task: BuildTask): string {
  if (task.status === "running" || task.status === "queued") return "正在执行";
  if (task.exitCode !== null) return `退出码 ${task.exitCode}`;
  return task.errorSummary ?? "—";
}

function spansFor(seq: number, text: string) {
  return ansiSpans(`${selectedId.value ?? "none"}:${seq}`, text);
}
</script>

<template>
  <div class="builds-page">
    <div class="toolbar">
      <n-select
        v-model:value="projects.currentId"
        :options="projectOptions"
        placeholder="选择项目"
        style="width: 220px"
        @update:value="(value: string) => projects.select(value)"
      />
      <n-button type="primary" :loading="builds.starting" @click="startBuild(projects.currentId)">
        启动构建
      </n-button>
      <n-button :disabled="!selected || (selected.status !== 'running' && selected.status !== 'queued')" @click="cancelBuild">
        取消
      </n-button>
      <div class="spacer" />
      <n-button quaternary @click="builds.load()">刷新列表</n-button>
    </div>

    <div class="builds-grid">
      <section class="card">
        <header class="card-head">
          <div>
            <div class="card-title">构建任务</div>
            <div class="card-desc">共 {{ builds.tasks.length }} 条记录</div>
          </div>
        </header>
        <div class="tasks-scroll">
          <table v-if="builds.tasks.length" class="table">
            <tbody>
              <tr
                v-for="task in builds.tasks"
                :key="task.id"
                :class="{ active: task.id === selectedId }"
                @click="selectedId = task.id"
              >
                <td>
                  <div class="cell-main mono" style="font-size: 12px">{{ task.command }}</div>
                  <div class="cell-sub">{{ formatTime(task.createdAt) }}</div>
                </td>
                <td class="task-meta">
                  <StatusPill :status="task.status" />
                  <div class="cell-sub">{{ formatDuration(task.durationMs) }}</div>
                </td>
              </tr>
            </tbody>
          </table>
          <EmptyState v-else title="暂无构建任务" desc="选择项目后点击「启动构建」" />
        </div>
      </section>

      <section class="card">
        <header class="card-head">
          <div class="head-text">
            <div class="card-title">构建详情</div>
            <div class="card-desc">
              <template v-if="selected">
                {{ statusSummary(selected) }} · {{ formatDuration(selected.durationMs) }} · 日志
                {{ formatBytes(selected.logSize) }}
              </template>
              <template v-else>选择左侧任务查看日志</template>
            </div>
          </div>
          <div class="card-actions">
            <n-button
              v-if="selected && selected.status === 'succeeded'"
              size="small"
              type="primary"
              :loading="creating"
              @click="createArtifact(selected)"
            >
              生成发布包
            </n-button>
            <n-button
              v-if="selected?.logPath"
              size="small"
              quaternary
              @click="builds.loadLogs(selected.id, true)"
            >
              重新加载
            </n-button>
          </div>
        </header>

        <div class="detail-body">
          <div v-if="selected" class="tag-list">
            <span class="chip mono">ID {{ selected.id.slice(0, 8) }}</span>
            <span class="chip mono" v-if="selected.logSha256">日志 {{ shortSha(selected.logSha256) }}</span>
            <span class="chip" v-if="selected.errorSummary">{{ selected.errorSummary }}</span>
          </div>

          <div class="console-head">
            <span class="console-hint">
              <template v-if="hiddenCount">仅显示最近 {{ MAX_RENDER }} 行（共 {{ lines.length }} 行）</template>
              <template v-else-if="lines.length">共 {{ lines.length }} 行</template>
              <template v-else>暂无输出</template>
            </span>
            <button
              type="button"
              class="follow"
              :class="{ on: autoScroll }"
              @click="toggleFollow"
            >
              <span class="follow-dot" />跟随最新
            </button>
          </div>

          <div ref="consoleEl" class="log-console" @scroll="onConsoleScroll">
            <div v-if="!lines.length" class="muted">暂无日志输出</div>
            <div v-for="line in visibleLines" :key="line.seq" class="log-line" :class="line.stream">
              <span class="seq">{{ line.seq }}</span>
              <span class="log-text">
                <template v-for="(span, index) in spansFor(line.seq, line.text)" :key="index">
                  <span v-if="span.style" :style="span.style">{{ span.text }}</span>
                  <template v-else>{{ span.text }}</template>
                </template>
              </span>
            </div>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.builds-page {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.builds-grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 348px) minmax(0, 1fr);
  gap: 16px;
  align-items: stretch;
}

.builds-grid > .card {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.tasks-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.tasks-scroll tr {
  cursor: pointer;
}

.tasks-scroll tbody tr.active {
  background: var(--bs-primary-soft-2);
  box-shadow: inset 3px 0 0 var(--bs-primary);
}

.task-meta {
  width: 104px;
  text-align: right;
  white-space: nowrap;
}

.head-text {
  min-width: 0;
}

.card-head .card-desc {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px 16px 16px;
}

.console-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-size: 11.5px;
  color: var(--bs-muted);
}

.console-hint {
  font-family: var(--bs-mono);
}

.follow {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 3px 9px;
  border: 1px solid var(--bs-border);
  border-radius: 999px;
  background: var(--bs-surface);
  color: var(--bs-muted);
  font-size: 11.5px;
  cursor: pointer;
  transition: color 150ms ease, border-color 150ms ease, background 150ms ease;
}

.follow:hover {
  color: var(--bs-primary);
  border-color: #bcd2fb;
}

.follow.on {
  color: var(--bs-primary);
  background: var(--bs-primary-soft);
  border-color: #bcd2fb;
}

.follow-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.7;
}

.log-console {
  flex: 1;
  min-height: 220px;
  height: auto;
  padding: 10px 12px;
}

.log-line {
  display: grid;
  grid-template-columns: 46px minmax(0, 1fr);
  column-gap: 10px;
}

.log-line .seq {
  margin-right: 0;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.log-text {
  white-space: pre-wrap;
  word-break: break-word;
}

@media (max-width: 1180px) {
  .builds-grid {
    grid-template-columns: minmax(0, 300px) minmax(0, 1fr);
  }
}
</style>
