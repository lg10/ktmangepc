import { ref } from "vue";

export interface ToastItem {
  id: number;
  title: string;
  description?: string;
  variant: "default" | "success" | "destructive";
}

const toasts = ref<ToastItem[]>([]);
let seq = 0;

export function useToast() {
  function toast(opts: {
    title: string;
    description?: string;
    variant?: ToastItem["variant"];
  }) {
    const id = ++seq;
    toasts.value.push({ id, variant: "default", ...opts });
    setTimeout(() => dismiss(id), 4000);
    return id;
  }

  function dismiss(id: number) {
    const idx = toasts.value.findIndex((t) => t.id === id);
    if (idx >= 0) toasts.value.splice(idx, 1);
  }

  return { toasts, toast, dismiss };
}
