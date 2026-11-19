<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { RouterLink, RouterView, useRoute, useRouter } from "vue-router";
import { NButton, NSpin, useMessage } from "naive-ui";
import AuroraMark from "@/components/AuroraMark.vue";
import NavIcon from "@/components/NavIcon.vue";
import { useAppStore } from "@/stores/app";
import { useProjectsStore } from "@/stores/projects";
import { useBuildsStore } from "@/stores/builds";
import { useReleasesStore } from "@/stores/releases";
import { errorText, isTauri } from "@/composables/useIpc";
import type { QuickState } from "@/types/ipc";

const route = useRoute();
const router = useRouter();
const message = useMessage();
const app = useAppStore();
const projects = useProjectsStore();
const builds = useBuildsStore();
const releases = useReleasesStore();

const bootError = ref<string | null>(null);

const navItems = [
  { name: "dashboard", label: "工作台", path: "/", icon: "dashboard" },
  { name: "projects", label: "项目", path: "/projects", icon: "projects" },
  { name: "builds", label: "构建任务", path: "/builds", icon: "builds" },
  { name: "artifacts", label: "发布包", path: "/artifacts", icon: "artifacts" },
  { name: "releases", label: "发布管理", path: "/releases", icon: "releases" },
  { name: "audit", label: "操作审计", path: "/audit", icon: "audit" },
  { name: "settings", label: "设置", path: "/settings", icon: "settings" },
];

const title = computed(() => (route.meta.title as string) ?? "Buildship");
const subtitle = computed(() => (route.meta.subtitle as string) ?? "");

const activeBuildCount = computed(() => builds.activeTask);
const canBuild = computed(() => Boolean(projects.current) && !builds.activeTask);

onMounted(async () => {
  try {
    await app.load();
  } catch (err) {
    bootError.value = errorText(err);
  }
  try {
    await projects.load();
  } catch {
    /* 项目列表失败不阻塞界面，页面内会再次提示 */
  }
  await builds.ensureBridge();
  await releases.ensureBridge();

  if (isTauri()) {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<QuickState>("quick://state", (event) => app.applyQuick(event.payload));
    await listen<{ level: string; message: string }>("toast://message", (event) => {
      const { level, message: text } = event.payload;
      if (level === "error") message.error(text);
      else if (level === "warning") message.warning(text);
      else message.info(text);
    });
    // 悬浮栏点击"发布包"等快捷入口时，主面板跟随跳转
    await listen<{ route: string }>("navigate://route", (event) => {
      router.push(event.payload.route);
    });
  }
});

async function quickBuild() {
  const project = projects.current;
  if (!project) {
    message.warning("请先在「项目」页添加并选中一个项目");
    router.push("/projects");
    return;
  }
  try {
    const task = await builds.start(project.id);
    message.success(`已启动构建：${project.name}`);
    await app.loadQuick();
    router.push(`/builds?task=${task.id}`);
  } catch (err) {
    message.error(errorText(err));
  }
}
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <div class="brand">
        <AuroraMark :size="30" />
        <div>
          <div class="brand-name">Buildship</div>
          <div class="brand-sub">FRONTEND RELEASE STUDIO</div>
        </div>
      </div>

      <nav class="sidebar-nav">
        <div class="nav-group-label">工作区</div>
        <RouterLink
          v-for="item in navItems.slice(0, 5)"
          :key="item.name"
          :to="item.path"
          class="nav-item"
        >
          <NavIcon :name="item.icon" />
          <span>{{ item.label }}</span>
          <span v-if="item.name === 'builds' && activeBuildCount" class="nav-badge">1</span>
        </RouterLink>

        <div class="nav-group-label">治理</div>
        <RouterLink
          v-for="item in navItems.slice(5)"
          :key="item.name"
          :to="item.path"
          class="nav-item"
        >
          <NavIcon :name="item.icon" />
          <span>{{ item.label }}</span>
        </RouterLink>
      </nav>

      <div class="sidebar-footer">
        <div>
          数据目录<br />
          <span
            class="mono footer-path"
            style="font-size: 11px"
            :title="app.bootstrap?.dataDir ?? ''"
          >
            {{ app.bootstrap?.dataDir ?? "初始化中…" }}
          </span>
        </div>
        <div style="margin-top: 8px">
          Schema v{{ app.bootstrap?.schemaVersion ?? "-" }} ·
          {{ app.bootstrap?.platform ?? "-" }}
        </div>
      </div>
    </aside>

    <section class="main">
      <header class="topbar">
        <div>
          <div class="topbar-title">{{ title }}</div>
          <div class="topbar-sub">{{ subtitle }}</div>
        </div>
        <div class="topbar-actions">
          <span v-if="projects.current" class="chip">
            当前项目 · {{ projects.current.name }}
          </span>
          <n-button type="primary" :disabled="!canBuild" @click="quickBuild">构建</n-button>
        </div>
      </header>

      <main class="content">
        <div v-if="bootError" class="card">
          <div class="card-body">
            <div class="empty-title">启动失败</div>
            <div class="empty-desc">{{ bootError }}</div>
            <n-button style="margin-top: 12px" @click="app.load()">重试</n-button>
          </div>
        </div>
        <div v-else-if="!app.ready" style="display: grid; place-items: center; padding: 80px">
          <n-spin size="large" />
        </div>
        <RouterView v-else />
      </main>
    </section>
  </div>
</template>
