<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import { NButton, NInput, NModal, NSelect, NSwitch, useMessage } from "naive-ui";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import StatusPill from "@/components/StatusPill.vue";
import EmptyState from "@/components/EmptyState.vue";
import { errorText } from "@/composables/useIpc";
import { formatRelative } from "@/composables/useFormat";
import { useProjectsStore } from "@/stores/projects";
import { useBuildsStore } from "@/stores/builds";
import { useAppStore } from "@/stores/app";
import type { BuildProfile, PackageManager, Project, ScanResult } from "@/types/ipc";

const router = useRouter();
const message = useMessage();
const store = useProjectsStore();
const builds = useBuildsStore();
const app = useAppStore();

const addOpen = ref(false);
const editOpen = ref(false);
const scanning = ref(false);
const saving = ref(false);

const scan = ref<ScanResult | null>(null);
const addForm = reactive({
  path: "",
  name: "",
  packageManager: "npm" as PackageManager,
  buildScript: "",
  outputDir: "dist",
});

const editForm = reactive({
  id: "",
  name: "",
  packageManager: "npm" as PackageManager,
  packageManagerVersion: "",
  buildScript: "",
  buildArgs: "",
  outputDir: "dist",
  envText: "",
});

const profiles = ref<BuildProfile[]>([]);

const pmOptions = [
  { label: "npm", value: "npm" },
  { label: "pnpm", value: "pnpm" },
  { label: "yarn", value: "yarn" },
];

const scriptOptions = computed(() =>
  (scan.value?.scripts ?? []).map((name) => ({ label: name, value: name })),
);

const outputOptions = computed(() => {
  const list = scan.value?.candidateOutputDirs ?? [];
  const merged = new Set([...list, "dist", "build"]);
  return Array.from(merged).map((value) => ({ label: value, value }));
});

onMounted(async () => {
  await store.load();
});

async function pickDirectory() {
  try {
    const selected = await openDialog({ directory: true, multiple: false, title: "选择前端项目目录" });
    if (typeof selected === "string") {
      await runScan(selected);
    }
  } catch (err) {
    message.error(errorText(err));
  }
}

async function runScan(path: string) {
  scanning.value = true;
  try {
    const result = await store.scan(path);
    scan.value = result;
    addForm.path = result.path;
    addForm.name = result.name;
    addForm.packageManager = result.packageManager;
    addForm.buildScript = result.suggestedScript ?? "";
    addForm.outputDir = result.suggestedOutputDir;
    addOpen.value = true;
    for (const warning of result.warnings) {
      message.warning(warning);
    }
  } catch (err) {
    message.error(errorText(err));
  } finally {
    scanning.value = false;
  }
}

async function submitAdd() {
  if (!addForm.path) {
    message.warning("请先选择项目目录");
    return;
  }
  saving.value = true;
  try {
    await store.add({
      path: addForm.path,
      name: addForm.name || null,
      packageManager: addForm.packageManager,
      outputDir: addForm.outputDir || null,
    });
    // 若用户改过构建脚本，补一次配置更新
    const project = store.current;
    if (project) {
      await store.update({
        id: project.id,
        name: addForm.name || project.name,
        packageManager: addForm.packageManager,
        packageManagerVersion: project.packageManagerVersion,
        buildScript: addForm.buildScript || null,
        buildArgs: [],
        outputDir: addForm.outputDir,
        env: {},
      });
    }
    message.success("项目已添加");
    addOpen.value = false;
    scan.value = null;
    app.loadQuick();
  } catch (err) {
    message.error(errorText(err));
  } finally {
    saving.value = false;
  }
}

async function openEdit(project: Project) {
  const list = await store.loadProfiles(project.id);
  profiles.value = list;
  const profile = list[0];
  editForm.id = project.id;
  editForm.name = project.name;
  editForm.packageManager = project.packageManager;
  editForm.packageManagerVersion = project.packageManagerVersion ?? "";
  editForm.buildScript = profile?.buildScript ?? "";
  editForm.buildArgs = "";
  editForm.outputDir = profile?.outputDir ?? "dist";
  editForm.envText = envToText(profile?.envJson);
  editOpen.value = true;
}

function envToText(envJson?: string): string {
  if (!envJson) return "";
  try {
    const parsed = JSON.parse(envJson) as Record<string, string>;
    return Object.entries(parsed)
      .map(([key, value]) => `${key}=${value}`)
      .join("\n");
  } catch {
    return "";
  }
}

function textToEnv(text: string): Record<string, string> {
  const result: Record<string, string> = {};
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const index = trimmed.indexOf("=");
    if (index <= 0) continue;
    result[trimmed.slice(0, index).trim()] = trimmed.slice(index + 1).trim();
  }
  return result;
}

async function submitEdit() {
  saving.value = true;
  try {
    await store.update({
      id: editForm.id,
      name: editForm.name,
      packageManager: editForm.packageManager,
      packageManagerVersion: editForm.packageManagerVersion || null,
      buildScript: editForm.buildScript || null,
      buildArgs: editForm.buildArgs
        ? editForm.buildArgs.split(/\s+/).filter(Boolean)
        : [],
      outputDir: editForm.outputDir,
      env: textToEnv(editForm.envText),
    });
    message.success("项目配置已保存");
    editOpen.value = false;
  } catch (err) {
    message.error(errorText(err));
  } finally {
    saving.value = false;
  }
}

async function archive(project: Project) {
  try {
    await store.archive(project.id);
    message.success(`已归档 ${project.name}`);
    app.loadQuick();
  } catch (err) {
    message.error(errorText(err));
  }
}

async function buildNow(project: Project) {
  try {
    await builds.start(project.id);
    message.success(`已启动构建：${project.name}`);
    app.loadQuick();
    router.push("/builds");
  } catch (err) {
    message.error(errorText(err));
  }
}
</script>

<template>
  <div>
    <div class="toolbar">
      <n-button type="primary" :loading="scanning" @click="pickDirectory">添加项目</n-button>
      <n-button quaternary @click="store.load()">刷新</n-button>
      <div class="spacer" />
      <span class="muted" style="font-size: 12px">
        扫描只读取 package.json 与 lockfile，不会执行任何项目脚本
      </span>
    </div>

    <div class="card">
      <div class="card-head">
        <div>
          <div class="card-title">项目列表</div>
          <div class="card-desc">共 {{ store.items.length }} 个活动项目</div>
        </div>
      </div>
      <div class="card-body tight">
        <table v-if="store.items.length" class="table">
          <thead>
            <tr>
              <th>项目</th>
              <th>包管理器</th>
              <th>Node</th>
              <th>框架</th>
              <th>最近构建</th>
              <th style="text-align: right">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="project in store.items"
              :key="project.id"
              :style="store.currentId === project.id ? 'background: var(--bs-primary-soft-2)' : ''"
            >
              <td>
                <div class="cell-main">{{ project.name }}</div>
                <div class="cell-sub mono">{{ project.path }}</div>
              </td>
              <td>
                <span class="chip mono">
                  {{ project.packageManager }} {{ project.packageManagerVersion ?? "" }}
                </span>
              </td>
              <td class="muted">{{ project.nodeVersion ?? "-" }}</td>
              <td class="muted">{{ project.framework ?? "-" }}</td>
              <td>
                <StatusPill :status="project.lastBuildStatus" />
                <div class="cell-sub">{{ formatRelative(project.lastBuildAt) }}</div>
              </td>
              <td>
                <div class="row-actions">
                  <n-button size="tiny" @click="store.select(project.id)">选中</n-button>
                  <n-button size="tiny" @click="buildNow(project)">构建</n-button>
                  <n-button size="tiny" quaternary @click="openEdit(project)">配置</n-button>
                  <n-button size="tiny" quaternary type="error" @click="archive(project)">归档</n-button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
        <EmptyState
          v-else
          title="还没有项目"
          desc="点击「添加项目」选择本地前端项目目录，Buildship 会自动识别包管理器、构建脚本与产物目录"
        >
          <n-button size="small" type="primary" :loading="scanning" @click="pickDirectory">添加项目</n-button>
        </EmptyState>
      </div>
    </div>

    <!-- 添加项目 -->
    <n-modal
      v-model:show="addOpen"
      preset="card"
      title="添加项目"
      style="width: 620px"
      :bordered="false"
    >
      <div class="form-grid">
        <div class="full">
          <label class="field-label">项目目录</label>
          <n-input v-model:value="addForm.path" placeholder="C:\\work\\my-web" readonly />
        </div>
        <div>
          <label class="field-label">项目名称</label>
          <n-input v-model:value="addForm.name" placeholder="项目显示名" />
        </div>
        <div>
          <label class="field-label">包管理器</label>
          <n-select v-model:value="addForm.packageManager" :options="pmOptions" />
        </div>
        <div>
          <label class="field-label">构建脚本</label>
          <n-select
            v-model:value="addForm.buildScript"
            :options="scriptOptions"
            filterable
            tag
            placeholder="build"
          />
          <div class="field-hint">
            对应 package.json 的 scripts；执行时使用参数数组启动，不拼接 shell 命令
          </div>
        </div>
        <div>
          <label class="field-label">产物目录</label>
          <n-select v-model:value="addForm.outputDir" :options="outputOptions" filterable tag />
          <div class="field-hint">相对项目根目录，例如 dist、build</div>
        </div>
        <div v-if="scan" class="full">
          <div class="tag-list">
            <span class="chip">框架：{{ scan.framework ?? "未识别" }}</span>
            <span class="chip mono">Node：{{ scan.nodeVersion ?? "-" }}</span>
            <span class="chip mono">Git：{{ scan.gitCommit ? scan.gitCommit.slice(0, 8) : "无" }}</span>
          </div>
        </div>
      </div>
      <template #footer>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button @click="addOpen = false">取消</n-button>
          <n-button type="primary" :loading="saving" @click="submitAdd">添加到项目列表</n-button>
        </div>
      </template>
    </n-modal>

    <!-- 编辑项目 -->
    <n-modal
      v-model:show="editOpen"
      preset="card"
      title="项目构建配置"
      style="width: 620px"
      :bordered="false"
    >
      <div class="form-grid">
        <div>
          <label class="field-label">项目名称</label>
          <n-input v-model:value="editForm.name" />
        </div>
        <div>
          <label class="field-label">包管理器</label>
          <n-select v-model:value="editForm.packageManager" :options="pmOptions" />
        </div>
        <div>
          <label class="field-label">构建脚本</label>
          <n-input v-model:value="editForm.buildScript" placeholder="build" />
        </div>
        <div>
          <label class="field-label">附加参数</label>
          <n-input v-model:value="editForm.buildArgs" placeholder="--mode production" />
          <div class="field-hint">空格分隔；参数会逐个传给包管理器，禁止包含 shell 元字符</div>
        </div>
        <div>
          <label class="field-label">产物目录</label>
          <n-input v-model:value="editForm.outputDir" placeholder="dist" />
        </div>
        <div>
          <label class="field-label">包管理器版本（记录用）</label>
          <n-input v-model:value="editForm.packageManagerVersion" placeholder="10.9.7" />
        </div>
        <div class="full">
          <label class="field-label">构建环境变量</label>
          <n-input
            v-model:value="editForm.envText"
            type="textarea"
            :autosize="{ minRows: 4, maxRows: 8 }"
            placeholder="NODE_OPTIONS=--max-old-space-size=4096"
          />
          <div class="field-hint">
            每行一条 KEY=VALUE。这些变量只在本机构建进程中生效，不会写入发布包。
          </div>
        </div>
      </div>
      <template #footer>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button @click="editOpen = false">取消</n-button>
          <n-button type="primary" :loading="saving" @click="submitEdit">保存</n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>
