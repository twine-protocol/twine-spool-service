<script lang="ts">
  import {
    Header,
    HeaderNav,
    HeaderNavItem,
    HeaderNavMenu,
    SideNav,
    SideNavItems,
    SideNavMenu,
    SideNavMenuItem,
    SideNavLink,
    SideNavDivider,
    SkipToContent,
    Content,
    Grid,
    Row,
    Column,
  } from "carbon-components-svelte"
  import "carbon-components-svelte/css/g90.css"
  import { afterNavigate } from "$app/navigation"

  const navItems = [
    { text: 'Dashboard', href: '#/' },
    { text: 'Api Keys', href: '#/apikeys' }
  ]

  let isSideNavOpen = false
  let route = $state('')

  afterNavigate((nav) => {
    route = nav.to?.url.hash ?? ''
    console.log(route)
  })

</script>

<main>
  <Header company="Twine" platformName="Spool Admin Panel" bind:isSideNavOpen>
    <svelte:fragment slot="skip-to-content">
      <SkipToContent />
    </svelte:fragment>
    <HeaderNav>
      {#each navItems as props}
      <HeaderNavItem {...props} isSelected={route == (props.href)} />
      {/each}
    </HeaderNav>
  </Header>

  <!-- <SideNav bind:isOpen={isSideNavOpen}>
    <SideNavItems>
      <SideNavLink icon={Fade} text="Link 1" href="/" isSelected />
      <SideNavLink icon={Fade} text="Link 2" href="/" />
      <SideNavLink icon={Fade} text="Link 3" href="/" />
      <SideNavMenu icon={Fade} text="Menu">
        <SideNavMenuItem href="/" text="Link 1" />
        <SideNavMenuItem href="/" text="Link 2" />
        <SideNavMenuItem href="/" text="Link 3" />
      </SideNavMenu>
      <SideNavDivider />
      <SideNavLink icon={Fade} text="Link 4" href="/" />
    </SideNavItems>
  </SideNav> -->

  <Content>
    <Grid>
      <Row>
        <Column>
          <slot />
        </Column>
      </Row>
    </Grid>
  </Content>
</main>
