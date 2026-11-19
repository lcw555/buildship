<script setup lang="ts">
/**
 * Aurora 悬浮快捷操作栏。
 *
 * 收起：只显示 Aurora Loop 图标 + 右下角在线状态点；
 * 悬停：向左展开玻璃胶囊，四个操作依次淡入；
 * 离开热区 320ms 后收起（给用户跨越图标与操作栏的余量）。
 */
import { computed, onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { emit } from "@tauri-apps/api/event";
import AuroraMark from "@/components/AuroraMark.vue";
import { call, errorText, isTauri } from "@/composables/useIpc";
import type { BuildEvent, BuildStatus, QuickState } from "@/types/ipc";

const expanded = ref(false);
const quick = ref<QuickState>({
  hasProject: false,
  activeBuild: false,
  activeTaskId: null,
  projectName: null,
  projectId: null,
  lastBuildStatus: null,
});
const hint = ref<string | null>(null);
let collapseTimer: number | undefined;
let unlisten: (() => void) | undefined;

const dotClass = computed(() => {
  if (quick.value.activeBuild) return "busy";
  if (quick.value.lastBuildStatus === "failed") return "error";
  return quick.value.hasProject ? "online" : "idle";
});

const disabledBuild = computed(() => !quick.value.hasProject || quick.value.activeBuild);
const disabledStop = computed(() => !quick.value.activeBuild);

function expand() {
  if (collapseTimer) {
    window.clearTimeout(collapseTimer);
    collapseTimer = undefined;
  }
  if (expanded.value) return;
  expanded.value = true;
  void call("floating_expand").catch(() => undefined);
}

function scheduleCollapse() {
  if (collapseTimer) window.clearTimeout(collapseTimer);
  collapseTimer = window.setTimeout(() => {
    expanded.value = false;
    void call("floating_collapse").catch(() => undefined);
  }, 320);
}

function flash(text: string) {
  hint.value = text;
  window.setTimeout(() => {
    if (hint.value === text) hint.value = null;
  }, 2400);
}

async function startBuild() {
  if (disabledBuild.value || !quick.value.projectId) {
    flash(quick.value.hasProject ? "已有构建在进行" : "请先在面板中选择项目");
    return;
  }
  try {
    await call("build_start", { projectId: quick.value.projectId });
    flash("构建已启动");
  } catch (err) {
    flash(errorText(err));
  }
}

async function stopBuild() {
  if (!quick.value.activeTaskId) return;
  try {
    await call("build_cancel", { taskId: quick.value.activeTaskId });
    flash("已发送取消指令");
  } catch (err) {
    flash(errorText(err));
  }
}

async function openPanel(route?: string) {
  if (route) {
    // 主面板监听该事件后跳转
    await emit("navigate://route", { route }).catch(() => undefined);
  }
  await call("window_show_main").catch(() => undefined);
}

async function startDrag(event: MouseEvent) {
  if (event.button !== 0) return;
  if (!isTauri()) return;
  try {
    await getCurrentWindow().startDragging();
  } catch {
    /* 忽略：部分平台不允许无边框窗口拖拽 */
  }
}

function applyQuick(next: QuickState) {
  quick.value = next;
}

onMounted(async () => {
  try {
    quick.value = await call<QuickState>("quick_state_get");
  } catch {
    /* 桌面端未就绪时忽略 */
  }
  if (isTauri()) {
    const { listen } = await import("@tauri-apps/api/event");
    unlisten = await listen<QuickState>("quick://state", (event) => applyQuick(event.payload));
    await listen<BuildEvent>("build://status", (event) => {
      const status: BuildStatus = event.payload.status;
      if (status === "running" || status === "queued") {
        quick.value = { ...quick.value, activeBuild: true, activeTaskId: event.payload.taskId };
      } else {
        quick.value = { ...quick.value, activeBuild: false, activeTaskId: null, lastBuildStatus: status };
      }
    });
  }
});

onUnmounted(() => {
  if (collapseTimer) window.clearTimeout(collapseTimer);
  unlisten?.();
});
</script>

<template>
  <div class="dock" @mouseenter="expand" @mouseleave="scheduleCollapse">
    <transition name="capsule">
      <div v-if="expanded" class="capsule">
        <button class="action" style="--i: 0" :disabled="disabledBuild" title="构建当前项目" @click="startBuild">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
            <path d="M8 5v14l11-7z" />
          </svg>
          <span>构建</span>
        </button>
        <button class="action" style="--i: 1" :disabled="disabledStop" title="停止当前构建" @click="stopBuild">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
            <rect x="6" y="6" width="12" height="12" rx="2" />
          </svg>
          <span>停止</span>
        </button>
        <button class="action" style="--i: 2" title="打开发布包视图" @click="openPanel('/artifacts')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round">
            <path d="M12 3 3 7.5v9L12 21l9-4.5v-9L12 3Z" />
            <path d="M3 7.5 12 12l9-4.5M12 12v9" />
          </svg>
          <span>发布包</span>
        </button>
        <button class="action" style="--i: 3" title="打开或收起主面板" @click="openPanel()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
            <rect x="3" y="4" width="18" height="16" rx="3" />
            <path d="M3 9h18M8 9v11" />
          </svg>
          <span>面板</span>
        </button>
        <div class="divider" />
      </div>
    </transition>

    <div class="orb" :class="{ busy: quick.activeBuild }" @mousedown="startDrag">
      <AuroraMark :size="50" :animated="true" />
      <span class="dot" :class="dotClass" />
      <transition name="hint">
        <div v-if="hint" class="hint">{{ hint }}</div>
      </transition>
    </div>
  </div>
</template>

<style scoped>
.dock {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  height: 100vh;
  width: 100vw;
  padding: 12px;
  box-sizing: border-box;
  background: transparent;
}

.capsule {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 62px;
  padding: 0 10px;
  border-radius: 31px;
  /* 不透明面板：悬浮窗是透明窗口，底下的桌面深浅不可控，
     半透明底色会让按钮直接糊在壁纸上看不清，这里用实心白底。 */
  background: linear-gradient(180deg, #ffffff 0%, #f3f8ff 100%);
  border: 1px solid rgba(255, 255, 255, 0.9);
  box-shadow: 0 10px 30px rgba(20, 45, 90, 0.22), 0 0 0 1px rgba(120, 150, 200, 0.12);
}

.action {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  width: 58px;
  height: 50px;
  border: none;
  border-radius: 14px;
  background: transparent;
  color: #22314a;
  font-size: 11px;
  font-family: var(--bs-font);
  cursor: pointer;
  transition: background 160ms ease, color 160ms ease, transform 160ms ease;
  animation: rise 240ms ease both;
  animation-delay: calc(var(--i) * 32ms);
}

.action svg {
  width: 19px;
  height: 19px;
}

.action:hover:not(:disabled) {
  background: rgba(47, 107, 255, 0.12);
  color: #1f57e0;
  transform: translateY(-1px);
}

.action:active:not(:disabled) {
  transform: scale(0.96);
}

.action:disabled {
  color: #9aa8bd;
  cursor: not-allowed;
}

.divider {
  width: 1px;
  height: 32px;
  margin: 0 4px;
  background: rgba(20, 45, 90, 0.12);
}

.orb {
  position: relative;
  width: 62px;
  height: 62px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  cursor: grab;
  transition: transform 200ms ease, filter 200ms ease;
  filter: drop-shadow(0 8px 18px rgba(28, 68, 140, 0.28));
}

.orb:hover {
  transform: scale(1.06);
  filter: drop-shadow(0 10px 24px rgba(28, 68, 140, 0.36));
}

.orb:active {
  cursor: grabbing;
  transform: scale(0.97);
}

.orb.busy::after {
  content: "";
  position: absolute;
  inset: -4px;
  border-radius: 50%;
  border: 2px solid rgba(47, 107, 255, 0.35);
  border-top-color: rgba(47, 107, 255, 0.95);
  animation: spin 1.1s linear infinite;
}

.dot {
  position: absolute;
  right: 3px;
  bottom: 3px;
  width: 11px;
  height: 11px;
  border-radius: 50%;
  border: 2px solid #ffffff;
  background: #9fb0c6;
  box-shadow: 0 1px 4px rgba(22, 35, 58, 0.25);
}

.dot.online {
  background: #17a673;
}

.dot.busy {
  background: #2f6bff;
  animation: pulse 1.4s ease-in-out infinite;
}

.dot.error {
  background: #e5484d;
}

.hint {
  position: absolute;
  bottom: 70px;
  right: 0;
  padding: 6px 10px;
  border-radius: 9px;
  background: rgba(22, 35, 58, 0.9);
  color: #fff;
  font-size: 11.5px;
  white-space: nowrap;
  pointer-events: none;
}

.capsule-enter-active,
.capsule-leave-active {
  transition: opacity 200ms ease, transform 240ms cubic-bezier(0.22, 0.61, 0.36, 1);
  transform-origin: right center;
}

.capsule-enter-from,
.capsule-leave-to {
  opacity: 0;
  transform: translateX(14px) scaleX(0.94);
}

.hint-enter-active,
.hint-leave-active {
  transition: opacity 160ms ease;
}

.hint-enter-from,
.hint-leave-to {
  opacity: 0;
}

@keyframes rise {
  from {
    opacity: 0;
    transform: translateY(6px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

@keyframes pulse {
  0%,
  100% {
    box-shadow: 0 0 0 0 rgba(47, 107, 255, 0.5);
  }
  50% {
    box-shadow: 0 0 0 5px rgba(47, 107, 255, 0);
  }
}

@media (prefers-reduced-motion: reduce) {
  .action,
  .capsule-enter-active,
  .capsule-leave-active,
  .orb,
  .orb.busy::after,
  .dot.busy {
    animation: none !important;
    transition: none !important;
  }
}
</style>
