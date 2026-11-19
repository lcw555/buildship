import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { call, isTauri } from "@/composables/useIpc";
import type { BuildEvent, BuildLogChunk, BuildTask, LogLine } from "@/types/ipc";

const MAX_LINES = 5000;

/** 构建任务与实时日志 */
export const useBuildsStore = defineStore("builds", () => {
  const tasks = ref<BuildTask[]>([]);
  const loading = ref(false);
  const logLines = ref<Record<string, LogLine[]>>({});
  const starting = ref(false);
  const bridgeReady = ref(false);

  const activeTask = computed<BuildTask | null>(
    () => tasks.value.find((task) => task.status === "running" || task.status === "queued") ?? null,
  );

  async function load(projectId?: string, limit = 100) {
    loading.value = true;
    try {
      tasks.value = await call<BuildTask[]>("build_list", {
        query: { projectId: projectId ?? null, status: null, limit },
      });
    } finally {
      loading.value = false;
    }
  }

  function upsert(task: BuildTask) {
    const index = tasks.value.findIndex((item) => item.id === task.id);
    if (index >= 0) {
      tasks.value.splice(index, 1, task);
    } else {
      tasks.value.unshift(task);
    }
  }

  async function start(projectId: string) {
    starting.value = true;
    try {
      const task = await call<BuildTask>("build_start", { projectId });
      upsert(task);
      logLines.value[task.id] = [];
      return task;
    } finally {
      starting.value = false;
    }
  }

  async function cancel(taskId: string) {
    await call("build_cancel", { taskId });
  }

  async function refreshTask(taskId: string) {
    const task = await call<BuildTask>("build_get", { taskId });
    upsert(task);
    return task;
  }

  async function loadLogs(taskId: string, reset = false) {
    const current = reset ? [] : logLines.value[taskId] ?? [];
    // 首次打开只取末尾 2000 行：构建日志动辄上万行，从第 1 行读既慢又没用
    const chunk = await call<BuildLogChunk>("build_logs", {
      query: { taskId, offset: reset ? 0 : current.length, limit: 2000, tail: reset },
    });
    const merged = reset ? chunk.lines : current.concat(chunk.lines);
    logLines.value[taskId] = merged.slice(-MAX_LINES);
    return chunk;
  }

  function appendLine(taskId: string, line: LogLine) {
    const list = logLines.value[taskId] ?? [];
    list.push(line);
    if (list.length > MAX_LINES) {
      list.splice(0, list.length - MAX_LINES);
    }
    logLines.value[taskId] = list;
  }

  function handleEvent(payload: BuildEvent) {
    if (payload.line) {
      appendLine(payload.taskId, payload.line);
    }
    const existing = tasks.value.find((item) => item.id === payload.taskId);
    if (existing && existing.status !== payload.status) {
      existing.status = payload.status;
      existing.exitCode = payload.exitCode ?? existing.exitCode;
      if (payload.status !== "running" && payload.status !== "queued") {
        existing.errorSummary = payload.message ?? existing.errorSummary;
        existing.finishedAt = new Date().toISOString();
      }
      // 结束后拉一次完整记录，拿到耗时/日志哈希等字段
      if (payload.status !== "running" && payload.status !== "queued") {
        void refreshTask(payload.taskId).catch(() => undefined);
      }
    }
  }

  /** 只注册一次事件桥接（App.vue 启动时调用） */
  async function ensureBridge() {
    if (bridgeReady.value || !isTauri()) return;
    const { listen } = await import("@tauri-apps/api/event");
    await listen<BuildEvent>("build://status", (event) => handleEvent(event.payload));
    bridgeReady.value = true;
  }

  return {
    tasks,
    loading,
    logLines,
    starting,
    activeTask,
    load,
    start,
    cancel,
    cancelBuild: cancel,
    refreshTask,
    loadLogs,
    appendLine,
    handleEvent,
    ensureBridge,
  };
});
