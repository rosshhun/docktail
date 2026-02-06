<script lang="ts">
  import Router from 'svelte-spa-router';
  import Layout from './lib/layout/Layout.svelte';
  import Containers from './routes/Containers.svelte';
  import ContainerDetails from './routes/ContainerDetails.svelte';
  import Logs from './routes/Logs.svelte';
  import Agents from './routes/Agents.svelte';
  import AgentDetails from './routes/AgentDetails.svelte';
  import Cluster from './routes/Cluster.svelte';

  const routes = {
    '/': Containers,
    '/containers': Containers,
    '/containers/:id': ContainerDetails,
    '/logs/:id': Logs,
    '/agents': Agents,
    '/agents/:id': AgentDetails,
    '/cluster': Cluster,
  };
</script>

<Layout>
  <Router {routes} />
</Layout>
