<script setup lang="ts">
import { useData } from "vitepress";
import DefaultTheme from "vitepress/theme";
import OpenKaraHome from "./OpenKaraHome.vue";

const { frontmatter } = useData();
</script>

<template>
  <OpenKaraHome v-if="frontmatter.layout === 'openkara-home'" />
  <DefaultTheme.Layout v-else />
</template>
