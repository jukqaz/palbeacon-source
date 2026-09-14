<script lang="ts">
  import { get } from 'svelte/store';
  import {
    parsePlanningWorkspace,
    planningWorkspace,
    serializePlanningWorkspace,
    updatePlanningWorkspace,
    type PlanningWorkspaceState,
  } from './plan-workspace';

  interface Props {
    onImported: (state: PlanningWorkspaceState) => void;
  }

  let { onImported }: Props = $props();
  let message = $state<string | null>(null);
  let error = $state(false);

  const download = () => {
    const source = serializePlanningWorkspace(get(planningWorkspace));
    const url = URL.createObjectURL(new Blob([source], { type: 'application/json' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = 'palbeacon-plan.json';
    link.click();
    URL.revokeObjectURL(url);
    error = false;
    message = '계획 파일을 저장했습니다.';
  };

  const importFile = async (file: File | undefined) => {
    if (!file) return;
    try {
      const state = parsePlanningWorkspace(await file.text());
      updatePlanningWorkspace(state);
      onImported(state);
      error = false;
      message = '계획을 불러왔습니다.';
    } catch (reason) {
      error = true;
      message = reason instanceof Error ? reason.message : '계획 파일을 읽을 수 없습니다.';
    }
  };
</script>

<div class="plan-transfer" aria-label="계획 파일">
  <button type="button" onclick={download}>계획 내보내기</button>
  <label>
    <span>계획 가져오기</span>
    <input
      type="file"
      accept="application/json,.json"
      onchange={(event) => void importFile(event.currentTarget.files?.[0])}
    />
  </label>
  {#if message}<small class:error role={error ? 'alert' : 'status'}>{message}</small>{/if}
</div>

<style>
  .plan-transfer {
    display: flex;
    min-height: 44px;
    align-items: center;
    justify-content: flex-end;
    gap: 7px;
    margin-bottom: 8px;
  }

  button,
  label {
    display: inline-flex;
    min-height: 36px;
    align-items: center;
    padding-inline: 11px;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-soft);
    background: var(--surface);
    cursor: pointer;
    font-size: 0.72rem;
    font-weight: 760;
  }

  label {
    position: relative;
  }

  input {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  button:focus-visible,
  label:has(input:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  small {
    color: var(--success);
    font-size: 0.72rem;
  }

  small.error {
    color: var(--danger);
  }

  @media (max-width: 520px) {
    .plan-transfer {
      justify-content: flex-start;
      flex-wrap: wrap;
    }

    small {
      width: 100%;
    }
  }
</style>
