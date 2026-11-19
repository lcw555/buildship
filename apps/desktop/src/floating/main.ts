import { createApp } from "vue";
import FloatingBar from "./FloatingBar.vue";
import "../styles/tokens.css";

// html 也要标脏：只给 body 设透明，html 的背景色仍会铺满整个窗口
document.documentElement.classList.add("bs-floating-doc");
document.body.classList.add("bs-floating-body");
createApp(FloatingBar).mount("#floating");
