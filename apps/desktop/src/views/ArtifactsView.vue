<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { NButton, NInput, NModal, NSelect, useMessage } from "naive-ui";
import StatusPill from "@/components/StatusPill.vue";
import EmptyState from "@/components/EmptyState.vue";
import { errorText } from "@/composables/useIpc";
import { formatBytes, formatTime, shortSha } from "@/composables/useFormat";
import { useArtifactsStore } from "@/stores/artifacts";
import { useProjectsStore } from "@/stores/projects";
import { useReleasesStore } from "@/stores/releases";
import type { Artifact } from "@/types/ipc";

const message = useMessage();
const artifacts = useArtifactsStore();
const projects = useProjectsStore();
const releases = useReleasesStore();

const filterProject = ref<string | null>(null);
const releaseOpen = ref(false);
const releaseArtifact = ref<Artifact | null>(null);
const targetId = ref<string | null>(null);
const version = ref("");
const submitting = ref(false);

const projectOptions = computed(() => [
  { label: "全部项目", value: null as unknown as string },
  ...projects.items.map((item) => ({ label: item.name, value: item.id })),
]);

const targetOptions = computed(() =>
  releases.environments.flatMap((group) =>
    group.targets.map((target) => ({
      label: `${group.environment.name} · ${target.name}（${target.iisSite}）`,
      value: target.id,
    })),
  ),
);

onMounted(async () => {
  await projects.load();
  await artifacts.load(filterProject.value ?? undefined);
  await releases.loadEnvironments();
});

async function reload() {
  await artifacts.load(filterProject.value ?? undefined);
}

async function verify(artifact: Artifact) {
  try {
    const result = await artifacts.verify(artifact.id);
    if (result.ok) message.success(result.message);
    else message.error(result.message);
    await reload();
  } catch (err) {
    message.error(errorText(err));
  }
}

async function openDir(artifact: Artifact) {
  try {
    await artifacts.openDir(artifact.id);
  } catch (err) {
    message.error(errorText(err));
  }
}

async function remove(artifact: Artifact) {
  try {
    await artifacts.remove(artifact.id);
    message.success("已删除发布包文件");
  } catch (err) {
    message.error(errorText(err));
  }
}

function openRelease(artifact: Artifact) {
  releaseArtifact.value = artifact;
  targetId.value = targetOptions.value[0]?.value ?? null;
  version.value = "";
  releaseOpen.value = true;
}

async function submitRelease() {
  if (!releaseArtifact.value || !targetId.value) {
    message.warning("请选择部署目标");
    return;
  }
  submitting.value = true;
  try {
    await releases.create(releaseArtifact.value.id, targetId.value, version.value || undefined);
    message.success("发布任务已创建，正在上传并切换版本");
    releaseOpen.value = false;
  } catch (err) {
    message.error(errorText(err));
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <div>
    <div class="toolbar">
      <n-select
        v-model:value="filterProject"
        :options="projectOptions"
        placeholder="全部项目"
        style="width: 220px"
        @update:value="reload"
      />
      <n-button quaternary @click="reload">刷新</n-button>
      <div class="spacer" />
      <span class="muted" style="font-size: 12px">
        发布包只包含构建产物目录，默认排除 .env、密钥文件与 node_modules
      </span>
    </div>

    <div class="card">
      <div class="card-head">
        <div>
          <div class="card-title">发布包</div>
          <div class="card-desc">共 {{ artifacts.items.length }} 个可用包</div>
        </div>
      </div>
      <div class="card-body tight">
        <table v-if="artifacts.items.length" class="table">
          <thead>
            <tr>
              <th>文件</th>
              <th>项目</th>
              <th>大小</th>
              <th>文件数</th>
              <th>SHA-256</th>
              <th>创建时间</th>
              <th style="text-align: right">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="artifact in artifacts.items" :key="artifact.id">
              <td>
                <div class="cell-main mono">{{ artifact.fileName }}</div>
                <div class="cell-sub">
                  <StatusPill :status="artifact.status === 'ready' ? 'succeeded' : 'failed'" :label="artifact.status === 'ready' ? '可发布' : artifact.status" />
                </div>
              </td>
              <td class="muted">
                {{ projects.items.find((p) => p.id === artifact.projectId)?.name ?? artifact.projectId.slice(0, 8) }}
              </td>
              <td class="muted">{{ formatBytes(artifact.sizeBytes) }}</td>
              <td class="muted">{{ artifact.fileCount }}</td>
              <td class="mono muted" :title="artifact.sha256">{{ shortSha(artifact.sha256) }}</td>
              <td class="muted">{{ formatTime(artifact.createdAt) }}</td>
              <td>
                <div class="row-actions">
                  <n-button size="tiny" type="primary" :disabled="artifact.status !== 'ready'" @click="openRelease(artifact)">
                    发布
                  </n-button>
                  <n-button size="tiny" @click="verify(artifact)">校验</n-button>
                  <n-button size="tiny" quaternary @click="openDir(artifact)">目录</n-button>
                  <n-button size="tiny" quaternary type="error" @click="remove(artifact)">删除</n-button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
        <EmptyState
          v-else
          title="还没有发布包"
          desc="在「构建任务」页选择一次成功的构建，点击「生成发布包」即可完成 ZIP 归档、manifest 与 SHA-256 计算"
        />
      </div>
    </div>

    <n-modal v-model:show="releaseOpen" preset="card" title="发起发布" style="width: 560px" :bordered="false">
      <div class="form-grid">
        <div class="full">
          <label class="field-label">发布包</label>
          <div class="chip mono">{{ releaseArtifact?.fileName }}</div>
          <div class="field-hint">
            {{ formatBytes(releaseArtifact?.sizeBytes ?? 0) }} ·
            {{ releaseArtifact?.fileCount ?? 0 }} 个文件 · SHA-256
            {{ shortSha(releaseArtifact?.sha256) }}
          </div>
        </div>
        <div class="full">
          <label class="field-label">部署目标</label>
          <n-select v-model:value="targetId" :options="targetOptions" placeholder="选择环境与 IIS 站点" />
          <div v-if="!targetOptions.length" class="field-hint">
            还没有部署目标，请先到「发布管理」页添加环境与 IIS 站点
          </div>
        </div>
        <div class="full">
          <label class="field-label">版本号（可选）</label>
          <n-input v-model:value="version" placeholder="留空则按时间戳生成，例如 20261118-120001" />
        </div>
      </div>
      <template #footer>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button @click="releaseOpen = false">取消</n-button>
          <n-button type="primary" :loading="submitting" :disabled="!targetOptions.length" @click="submitRelease">
            开始发布
          </n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>
