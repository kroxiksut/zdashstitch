<script lang="ts">
  import { onMount } from 'svelte';

  import { loadHealth, type HealthResponse } from './lib/api';

  let health: HealthResponse | null = null;
  let error = '';

  onMount(async () => {
    try {
      health = await loadHealth();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Не удалось прочитать состояние';
    }
  });
</script>

<svelte:head>
  <meta
    name="description"
    content="ZDashstitch — подготовительный каркас локального конструктора панелей Zabbix"
  />
</svelte:head>

<main>
  <section class="shell" aria-labelledby="title">
    <p class="eyebrow">Zabbix dashboard builder</p>
    <h1 id="title">ZDashstitch</h1>
    <p class="lead">
      Подготовительный каркас локального GUI. Редактирование и публикация панелей пока не
      реализованы.
    </p>

    {#if health}
      <dl class="status" aria-label="Состояние локального приложения">
        <div><dt>Backend</dt><dd>{health.status}</dd></div>
        <div><dt>Стадия</dt><dd>{health.product_stage}</dd></div>
        <div><dt>Целевая ветка</dt><dd>Zabbix {health.target_zabbix_family}</dd></div>
        <div><dt>Запись в Zabbix</dt><dd>{health.zabbix_writes_enabled ? 'включена' : 'отключена'}</dd></div>
      </dl>
    {:else if error}
      <p class="error" role="alert">{error}</p>
    {:else}
      <p class="pending" aria-live="polite">Проверяем локальный backend…</p>
    {/if}
  </section>
</main>
