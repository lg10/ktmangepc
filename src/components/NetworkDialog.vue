<script setup lang="ts">
import { reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { api } from "@/lib/api";
import { useToast } from "@/components/ui/toast/use-toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import { Loader2 } from "lucide-vue-next";
import type { RcuDevice } from "@/types";

/** kind：ip = 仅 IP 配置；server = 仅服务器配置（报文含双侧参数，未编辑侧按设备当前值保持） */
const props = defineProps<{ device: RcuDevice | null; kind: "ip" | "server" }>();
const open = defineModel<boolean>("open", { default: false });

const { t } = useI18n();
const { toast } = useToast();

const form = reactive({
  ipFlag: "1",
  ip: "",
  mask: "255.255.255.0",
  gateway: "",
  dns: "223.5.5.5",
  serverFlag: "0",
  serverUrl: "",
  serverIp: "",
  serverPort: "9527",
});

function defaultGateway(ip: string) {
  const parts = ip.split(".");
  return parts.length === 4 ? `${parts[0]}.${parts[1]}.${parts[2]}.1` : "";
}

/** 设备当前 IP 侧结构化参数（与原版一致直接读 UdpModel，不解析显示文本） */
function deviceIpSide(d: RcuDevice) {
  // DHCP 设备上报的网关/掩码可能为 0.0.0.0，此时按 IP 推导默认值，避免输入框出现无效值
  const gw = d.gateway && d.gateway !== "0.0.0.0" ? d.gateway : defaultGateway(d.ip);
  const mask = d.mask && d.mask !== "0.0.0.0" ? d.mask : "255.255.255.0";
  return {
    ipFlag: d.ipFlag,
    ip: d.ip,
    mask,
    gateway: gw,
    dns: d.dns || "223.5.5.5",
  };
}

/** 设备当前服务器侧结构化参数 */
function deviceServerSide(d: RcuDevice) {
  return {
    serverFlag: d.serverFlag,
    serverUrl: d.serverFlag === 1 ? "" : d.serverUrl,
    // 原单设备路径 serverIp 恒随报文附带（rcu.ip），与当前模式无关，故始终保留设备值
    serverIp: d.serverIp,
    serverPort: d.serverPort,
  };
}

watch(open, (v) => {
  if (!v || !props.device) return;
  const ipSide = deviceIpSide(props.device);
  form.ipFlag = String(ipSide.ipFlag);
  form.ip = ipSide.ip;
  form.mask = ipSide.mask;
  form.gateway = ipSide.gateway;
  form.dns = ipSide.dns;
  const sv = deviceServerSide(props.device);
  form.serverFlag = String(sv.serverFlag);
  form.serverUrl = sv.serverUrl || "hotel.kingint.com";
  form.serverIp = sv.serverIp;
  form.serverPort = String(sv.serverPort || 9527);
});

const sending = ref(false);

async function send() {
  if (!props.device) return;
  sending.value = true;
  try {
    // 未编辑侧直接用设备结构化值保持原参数（原实现单设备路径同为 rcu.* 回填）
    const ipSide = deviceIpSide(props.device);
    const svSide = deviceServerSide(props.device);
    const ipPart =
      props.kind === "ip"
        ? {
            ipFlag: Number(form.ipFlag),
            ip: form.ip,
            mask: form.mask,
            gateway: form.gateway,
            dns: form.dns,
          }
        : { ipFlag: ipSide.ipFlag, ip: ipSide.ip, mask: ipSide.mask, gateway: ipSide.gateway, dns: ipSide.dns };
    const svPart =
      props.kind === "server"
        ? {
            serverFlag: Number(form.serverFlag),
            serverUrl: form.serverUrl,
            serverIp: form.serverIp,
            serverPort: Number(form.serverPort) || 0,
          }
        : svSide;
    await api.sendNetworkConfig(props.device.equipId, { ...ipPart, ...svPart });
    toast({
      title: t("common.success"),
      description:
        props.kind === "ip" ? "IP 配置指令已下发" : "服务器配置指令已下发",
      variant: "success",
    });
    open.value = false;
  } catch (e) {
    toast({ title: t("common.failed"), description: String(e), variant: "destructive" });
  } finally {
    sending.value = false;
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-md">
      <DialogTitle>{{ props.kind === "ip" ? "IP 配置" : "服务器配置" }}</DialogTitle>
      <p class="text-xs text-muted-foreground font-mono">
        {{ props.device?.equipId }} · {{ props.device?.ip }}
      </p>
      <p class="text-[11px] text-muted-foreground -mt-1">
        {{ props.kind === "ip" ? "仅下发 IP 侧参数，服务器侧保持设备当前值" : "仅下发服务器侧参数，IP 侧保持设备当前值" }}
      </p>

      <div class="grid gap-3 mt-2">
        <template v-if="props.kind === 'ip'">
          <div class="grid grid-cols-2 gap-3">
            <div class="space-y-1.5">
              <Label>{{ t("network.ipMode") }}</Label>
              <Select v-model="form.ipFlag">
                <SelectTrigger />
                <SelectContent>
                  <SelectItem value="1">{{ t("network.static") }}</SelectItem>
                  <SelectItem value="0">{{ t("network.dhcp") }}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div class="space-y-1.5">
              <Label>{{ t("network.ip") }}</Label>
              <Input
                v-model="form.ip"
                placeholder="192.168.1.100"
                :disabled="form.ipFlag === '0'"
              />
            </div>
          </div>

          <div class="grid grid-cols-2 gap-3">
            <div class="space-y-1.5">
              <Label>{{ t("network.mask") }}</Label>
              <Input v-model="form.mask" :disabled="form.ipFlag === '0'" />
            </div>
            <div class="space-y-1.5">
              <Label>{{ t("network.gateway") }}</Label>
              <Input v-model="form.gateway" :disabled="form.ipFlag === '0'" />
            </div>
          </div>

          <div class="space-y-1.5">
            <Label>{{ t("network.dns") }}</Label>
            <Input v-model="form.dns" />
          </div>
        </template>

        <template v-else>
          <div class="space-y-1.5">
            <Label>{{ t("network.serverMode") }}</Label>
            <Select v-model="form.serverFlag">
              <SelectTrigger />
              <SelectContent>
                <SelectItem value="0">域名方式</SelectItem>
                <SelectItem value="1">IP 方式</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <template v-if="form.serverFlag === '0'">
            <div class="grid grid-cols-2 gap-3">
              <div class="space-y-1.5">
                <Label>{{ t("network.serverUrl") }}</Label>
                <Input v-model="form.serverUrl" placeholder="hotel.kingint.com" />
              </div>
              <div class="space-y-1.5">
                <Label>{{ t("network.serverPort") }}</Label>
                <Input v-model="form.serverPort" type="number" min="0" max="9999" />
              </div>
            </div>
          </template>
          <template v-else>
            <div class="grid grid-cols-2 gap-3">
              <div class="space-y-1.5">
                <Label>{{ t("network.serverIp") }}</Label>
                <Input v-model="form.serverIp" />
              </div>
              <div class="space-y-1.5">
                <Label>{{ t("network.serverPort") }}</Label>
                <Input v-model="form.serverPort" type="number" min="0" max="9999" />
              </div>
            </div>
          </template>
        </template>
      </div>

      <div class="flex justify-end gap-2 mt-2">
        <Button variant="ghost" @click="open = false">{{ t("common.cancel") }}</Button>
        <Button :disabled="sending" @click="send">
          <Loader2 v-if="sending" class="h-4 w-4 animate-spin" />
          {{ sending ? t("network.sending") : t("network.send") }}
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
