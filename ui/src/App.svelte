<script lang="ts">
  import Router from 'svelte-spa-router';
  import Layout from './lib/layout/Layout.svelte';
  import Containers from './routes/Containers.svelte';
  import ContainerDetails from './routes/ContainerDetails.svelte';
  import Logs from './routes/Logs.svelte';
  import Agents from './routes/Agents.svelte';
  import AgentDetails from './routes/AgentDetails.svelte';
  import Cluster from './routes/Cluster.svelte';
  import SwarmOverview from './routes/SwarmOverview.svelte';
  import Services from './routes/Services.svelte';
  import ServiceDetails from './routes/ServiceDetails.svelte';
  import Nodes from './routes/Nodes.svelte';
  import NodeDetails from './routes/NodeDetails.svelte';
  import Stacks from './routes/Stacks.svelte';
  import StackDetails from './routes/StackDetails.svelte';
  import Networks from './routes/Networks.svelte';
  import SecretsConfigs from './routes/SecretsConfigs.svelte';

  const routes = {
    '/': Containers,
    '/containers': Containers,
    '/containers/:id': ContainerDetails,
    '/logs/:id': Logs,
    '/agents': Agents,
    '/agents/:id': AgentDetails,
    '/cluster': Cluster,
    '/swarm': SwarmOverview,
    '/swarm/services': Services,
    '/swarm/services/:id': ServiceDetails,
    '/swarm/nodes': Nodes,
    '/swarm/nodes/:id': NodeDetails,
    '/swarm/stacks': Stacks,
    '/swarm/stacks/:name': StackDetails,
    '/swarm/networks': Networks,
    '/swarm/secrets': SecretsConfigs,
  };
</script>

<Layout>
  <Router {routes} />
</Layout>
