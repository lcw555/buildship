<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { NButton, NSpin, useMessage } from "naive-ui";
import StatusPill from "@/components/StatusPill.vue";
import EmptyState from "@/components/EmptyState.vue";
import { call, errorText } from "@/composables/useIpc";
import { formatBytes, formatDuration, formatRelative, formatTime, shortSha } from "@/composables/useFormat";
import { useAppStore } from "@/stores/app";
import { useProjectsStore } from "@/stores/projects";
import { useBuildsStore } from "@/stores/builds";
import type { DashboardSummary } from "@/types/ipc";

const router = useRouter();
const message = useMessage();
const app = useAppStore();
const projects = useProjectsStore();
const builds = useBuildsStore();

const summary = ref<DashboardSummary | null>(null);
const loading = ref(false);

const nodeMissing = computed(() => app.bootstrap !== null && !app.bootstrap.toolchain.node.available);

onMounted(async () => {
  await refresh();
});

async function refresh() {
  loading.value = true;
  try {
    summary.value = await call<DashboardSummary>("dashboard_summary");
  } catch (err) {
    message.error(errorText(err));
  } finally {
    loading.value = false;
  }
}

async function buildNow(projectId: string) {
  try {
    await builds.start(projectId);
    message.success("构建已启动");
    await refresh();
    router.push("/builds");
  } catch (err) {
    message.error(errorText(err));
  }
}
</script>

<template>
  <div>
    <div v-if="nodeMissing" class="card" style="margin-bottom: 16px; border-color: #f6d9a8">
      <div class="card-body" style="display: flex; gap: 12px; align-items: flex-start">
        <div>
          <div class="card-title" style="color: var(--bs-warning)">未检测到 Node.js</div>
          <div class="card-desc">
            {{ app.bootstrap?.toolchain.node.hint }}
          </div>
        </div>
        <div class="spacer" />
        <n-button size="small" @click="router.push('/settings')">查看工具链</n-button>
      </div>
    </div>

    <div class="stat-grid">
      <div class="stat-card">
        <div class="stat-label">项目</div>
        <div class="stat-value">{{ summary?.projectCount ?? 0 }}</div>
        <div class="stat-foot">本地已纳管的前端项目</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">活动构建</div>
        <div class="stat-value">{{ summary?.activeBuildCount ?? 0 }}</div>
        <div class="stat-foot">同一项目仅允许一个并发构建</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">发布包</div>
        <div class="stat-value">{{ summary?.artifactCount ?? 0 }}</div>
        <div class="stat-foot">共 {{ formatBytes(summary?.totalArtifactBytes ?? 0) }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">线上版本</div>
        <div class="stat-value">{{ summary?.healthyReleaseCount ?? 0 }}</div>
        <div class="stat-foot">健康且当前生效的发布</div>
      </div>
    </div>

    <div class="grid-2" style="margin-top: 16px">
      <div class="card">
        <div class="card-head">
          <div>
            <div class="card-title">最近构建</div>
            <div class="card-desc">最新 6 条构建任务</div>
          </div>
          <div class="card-actions">
            <n-button size="small" quaternary @click="refresh">刷新</n-button>
            <n-button size="small" @click="router.push('/builds')">全部</n-button>
          </div>
        </div>
        <div class="card-body tight">
          <n-spin :show="loading">
            <table class="table">
              <thead>
                <tr>
                  <th>项目</th>
                  <th>状态</th>
                  <th>耗时</th>
                  <th>开始时间</th>
                </tr>
              </thead>
              <tbody v-if="summary?.recentBuilds.length">
                <tr v-for="task in summary.recentBuilds" :key="task.id">
                  <td>
                    <div class="cell-main">
                      {{ projects.items.find((p) => p.id === task.projectId)?.name ?? task.projectId.slice(0, 8) }}
                    </div>
                    <div class="cell-sub mono">{{ task.command }}</div>
                  </td>
                  <td><StatusPill :status="task.status" /></td>
                  <td class="muted">{{ formatDuration(task.durationMs) }}</td>
                  <td class="muted">{{ formatTime(task.startedAt ?? task.createdAt) }}</td>
                </tr>
              </tbody>
            </table>
            <EmptyState v-if="!summary?.recentBuilds.length" title="还没有构建记录" desc="在项目页选择项目后点击右上角「构建」即可开始" />
          </n-spin>
        </div>
      </div>

      <div class="card">
        <div class="card-head">
          <div>
            <div class="card-title">最近发布</div>
            <div class="card-desc">线上版本与最近一次部署结果</div>
          </div>
          <div class="card-actions">
            <n-button size="small" @click="router.push('/releases')">发布管理</n-button>
          </div>
        </div>
        <div class="card-body tight">
          <table class="table">
            <thead>
              <tr>
                <th>版本</th>
                <th>状态</th>
                <th>时间</th>
              </tr>
            </thead>
            <tbody v-if="summary?.recentReleases.length">
              <tr v-for="release in summary.recentReleases" :key="release.id">
                <td>
                  <div class="cell-main mono">{{ release.version }}</div>
                  <div class="cell-sub">{{ release.isCurrent ? "当前线上版本" : "历史版本" }}</div>
                </td>
                <td><StatusPill :status="release.status" /></td>
                <td class="muted">{{ formatRelative(release.createdAt) }}</td>
              </tr>
            </tbody>
          </table>
          <EmptyState v-if="!summary?.recentReleases.length" title="还没有发布记录" desc="生成发布包后可在「发布包」页发起发布" />
        </div>
      </div>
    </div>

    <div class="card" style="margin-top: 16px">
      <div class="card-head">
        <div>
          <div class="card-title">最近项目</div>
          <div class="card-desc">按最近更新时间排序</div>
        </div>
        <div class="card-actions">
          <n-button size="small" @click="router.push('/projects')">管理项目</n-button>
        </div>
      </div>
      <div class="card-body tight">
        <table v-if="summary?.recentProjects.length" class="table">
          <thead>
            <tr>
              <th>项目</th>
              <th>包管理器</th>
              <th>框架</th>
              <th>最近构建</th>
              <th style="text-align: right">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="project in summary.recentProjects" :key="project.id">
              <td>
                <div class="cell-main">{{ project.name }}</div>
                <div class="cell-sub mono">{{ project.path }}</div>
              </td>
              <td>
                <span class="chip mono">{{ project.packageManager }}</span>
              </td>
              <td class="muted">{{ project.framework ?? "-" }}</td>
              <td>
                <StatusPill :status="project.lastBuildStatus" />
              </td>
              <td>
                <div class="row-actions">
                  <n-button size="tiny" @click="buildNow(project.id)">构建</n-button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
        <EmptyState v-else title="尚未添加项目" desc="点击下方按钮添加本地前端项目目录">
          <n-button size="small" type="primary" @click="router.push('/projects')">添加项目</n-button>
        </EmptyState>
      </div>
    </div>

    <div v-if="summary && summary.recentBuilds[0]?.logSha256" class="muted" style="margin-top: 14px; font-size: 12px">
      最近一次构建日志摘要：<span class="mono">{{ shortSha(summary.recentBuilds[0].logSha256) }}</span>
    </div>
  </div>
</template>
