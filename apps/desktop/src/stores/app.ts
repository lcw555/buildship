import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { call } from "@/composables/useIpc";
import type { AppSetting, BootstrapInfo, QuickState, ToolchainReport } from "@/types/ipc";

const DEFAULT_QUICK: QuickState = {
  hasProject: false,
  activeBuild: false,
  activeTaskId: null,
  projectName: null,
  projectId: null,
  lastBuildStatus: null,
};

/** 窗口通常早于后端状态注册完成，启动竞态时按短间隔重试而不是立刻报错 */
const STARTUP_RETRIES = 25;
const STARTUP_RETRY_DELAY = 200;

/** 是否是「后端还没就绪」类错误（Tauri 的 state 尚未注册） */
function isNotReadyError(err: unknown): boolean {
  const text = err instanceof Error ? err.message : String(err);
  return /not managed|state not managed/i.test(text);
}

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** 全局应用状态：引导信息、工具链、设置、快捷栏状态 */
export const useAppStore = defineStore("app", () => {
  const bootstrap = ref<BootstrapInfo | null>(null);
  const loading = ref(false);
  const ready = ref(false);
  const error = ref<string | null>(null);
  const quick = ref<QuickState>({ ...DEFAULT_QUICK });

  const toolchain = computed<ToolchainReport | null>(() => bootstrap.value?.toolchain ?? null);
  const nodeReady = computed(() => bootstrap.value?.toolchain.node.available ?? false);

  const settingsMap = computed<Record<string, string>>(() => {
    const map: Record<string, string> = {};
    for (const item of bootstrap.value?.settings ?? []) {
      map[item.key] = item.value;
    }
    return map;
  });

  function setting(key: string, fallback = ""): string {
    return settingsMap.value[key] ?? fallback;
  }

  async function load() {
    loading.value = true;
    error.value = null;
    try {
      for (let attempt = 0; ; attempt += 1) {
        try {
          bootstrap.value = await call<BootstrapInfo>("system_bootstrap");
          ready.value = true;
          await loadQuick();
          return;
        } catch (err) {
          // 只有「状态未注册」才值得等，真实错误（比如数据库打不开）立刻抛给界面
          if (!isNotReadyError(err) || attempt >= STARTUP_RETRIES) throw err;
          await sleep(STARTUP_RETRY_DELAY);
        }
      }
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err);
      throw err;
    } finally {
      loading.value = false;
    }
  }

  async function refreshToolchain() {
    const report = await call<ToolchainReport>("system_toolchain");
    if (bootstrap.value) {
      bootstrap.value = { ...bootstrap.value, toolchain: report };
    }
    return report;
  }

  async function updateSettings(entries: Record<string, string>) {
    const settings = await call<AppSetting[]>("settings_update", { patch: { entries } });
    if (bootstrap.value) {
      bootstrap.value = { ...bootstrap.value, settings };
    }
  }

  async function loadQuick() {
    quick.value = await call<QuickState>("quick_state_get");
  }

  function applyQuick(next: QuickState) {
    quick.value = next;
  }

  return {
    bootstrap,
    loading,
    ready,
    error,
    quick,
    toolchain,
    nodeReady,
    settingsMap,
    setting,
    load,
    refreshToolchain,
    updateSettings,
    loadQuick,
    applyQuick,
  };
});
