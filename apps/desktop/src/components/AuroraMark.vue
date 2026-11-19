<script setup lang="ts">
/**
 * Buildship 白蓝液态玻璃光球。
 *
 * 素材来自「Buildship_白蓝液态玻璃图标_素材包」，用的是**透明背景版**：
 * 画布四角 Alpha=0，容器不能再铺任何底色，否则一圈白框又回来了。
 *
 * 动态感不靠 GIF：素材包里的循环动画 GIF 没有透明通道（transparency=None），
 * 直接用会在图标背后带一块方形底。这里改用 CSS 叠加两层动画：
 * 外层柔光呼吸 + 沿球体边缘缓慢流动的白/青高光，
 * 并用 `prefers-reduced-motion` 在系统要求减少动效时全部关掉。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import orbSrc from "@/assets/buildship-orb.png";

const props = withDefaults(
  defineProps<{
    size?: number;
    animated?: boolean;
    glow?: boolean;
  }>(),
  { size: 28, animated: true, glow: false },
);

const reducedMotion = ref(false);
let media: MediaQueryList | undefined;

function syncPreference(event?: MediaQueryListEvent) {
  reducedMotion.value = event ? event.matches : Boolean(media?.matches);
}

onMounted(() => {
  media = window.matchMedia("(prefers-reduced-motion: reduce)");
  syncPreference();
  media.addEventListener("change", syncPreference);
});

onBeforeUnmount(() => {
  media?.removeEventListener("change", syncPreference);
});

/** 只有显式要求动画、且系统未开启「减少动态效果」时才跑帧 */
const lively = computed(() => props.animated && !reducedMotion.value);
</script>

<template>
  <span
    class="aurora-mark"
    :class="{ glow, lively }"
    :style="{ width: `${size}px`, height: `${size}px` }"
  >
    <span class="orb-glow" aria-hidden="true" />
    <img :src="orbSrc" alt="Buildship" draggable="false" />
    <span class="orb-rim" aria-hidden="true" />
  </span>
</template>

<style scoped>
.aurora-mark {
  position: relative;
  display: inline-grid;
  place-items: center;
  flex: none;
  line-height: 0;
  /* 透明素材：任何底色都会破坏 Alpha 效果 */
  background: transparent;
}

.aurora-mark img {
  position: relative;
  z-index: 1;
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
  pointer-events: none;
  user-select: none;
}

/* 球体外围的冷色柔光，呼吸式明暗 */
.orb-glow {
  position: absolute;
  inset: -12%;
  z-index: 0;
  border-radius: 50%;
  background: radial-gradient(
    circle at 50% 50%,
    rgba(93, 174, 255, 0.42) 0%,
    rgba(118, 231, 248, 0.18) 52%,
    rgba(223, 244, 255, 0) 72%
  );
  filter: blur(4px);
  opacity: 0.9;
  pointer-events: none;
}

/* 沿边缘流动的高光：一段白、一段青，用 mask 收成细环 */
.orb-rim {
  position: absolute;
  inset: 1%;
  z-index: 2;
  border-radius: 50%;
  background: conic-gradient(
    from 0deg,
    rgba(255, 255, 255, 0) 0deg,
    rgba(255, 255, 255, 0.9) 36deg,
    rgba(118, 231, 248, 0.55) 78deg,
    rgba(255, 255, 255, 0) 132deg,
    rgba(255, 255, 255, 0) 236deg,
    rgba(223, 244, 255, 0.55) 292deg,
    rgba(255, 255, 255, 0) 340deg
  );
  -webkit-mask: radial-gradient(
    circle at 50% 50%,
    transparent 0 45%,
    #000 47% 50%,
    transparent 52%
  );
  mask: radial-gradient(circle at 50% 50%, transparent 0 45%, #000 47% 50%, transparent 52%);
  filter: blur(1.5px);
  mix-blend-mode: screen;
  opacity: 0;
  pointer-events: none;
}

.aurora-mark.glow img {
  filter: drop-shadow(0 6px 16px rgba(47, 107, 255, 0.32));
}

/* 动画只在开启时生效，减少动效偏好下保持完全静态 */
.aurora-mark.lively .orb-glow {
  animation: orb-breathe 4.8s ease-in-out infinite;
}

.aurora-mark.lively .orb-rim {
  opacity: 0.5;
  animation: rim-sweep 9s linear infinite;
}

@keyframes orb-breathe {
  0%,
  100% {
    opacity: 0.75;
    transform: scale(0.98);
  }
  50% {
    opacity: 1;
    transform: scale(1.04);
  }
}

@keyframes rim-sweep {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}
</style>
