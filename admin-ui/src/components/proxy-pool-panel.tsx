import { useState } from 'react'
import { Plus, Pencil, Trash2, Activity, PowerOff, Wifi, WifiOff, Clock } from 'lucide-react'
import { toast } from 'sonner'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { Input } from '@/components/ui/input'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  useProxyPool,
  useAddProxy,
  useUpdateProxy,
  useDeleteProxy,
  useSetProxyEnabled,
  useCheckProxy,
} from '@/hooks/use-credentials'
import { extractErrorMessage } from '@/lib/utils'
import type { ProxyPoolEntry, AddProxyRequest, UpdateProxyRequest } from '@/types/api'

export function ProxyPoolPanel() {
  const [addDialogOpen, setAddDialogOpen] = useState(false)
  const [editEntry, setEditEntry] = useState<ProxyPoolEntry | null>(null)
  const [addForm, setAddForm] = useState<AddProxyRequest>({ name: '', url: '' })
  const [editForm, setEditForm] = useState<UpdateProxyRequest>({})

  const { data: proxies, isLoading } = useProxyPool()
  const { mutate: addProxyMut, isPending: isAdding } = useAddProxy()
  const { mutate: updateProxyMut, isPending: isUpdating } = useUpdateProxy()
  const { mutate: deleteProxyMut } = useDeleteProxy()
  const { mutate: setEnabledMut } = useSetProxyEnabled()
  const { mutate: checkProxyMut, isPending: isChecking } = useCheckProxy()

  const totalCount = proxies?.length ?? 0
  const healthyCount = proxies?.filter(p => p.enabled && p.healthy).length ?? 0
  const unhealthyCount = proxies?.filter(p => p.enabled && !p.healthy).length ?? 0
  const disabledCount = proxies?.filter(p => !p.enabled).length ?? 0

  const handleAdd = () => {
    if (!addForm.name.trim() || !addForm.url.trim()) {
      toast.error('名称和 URL 不能为空')
      return
    }
    addProxyMut(addForm, {
      onSuccess: () => {
        toast.success('代理已添加')
        setAddDialogOpen(false)
        setAddForm({ name: '', url: '' })
      },
      onError: (err) => toast.error(`添加失败: ${extractErrorMessage(err)}`),
    })
  }

  const handleEdit = () => {
    if (!editEntry) return
    updateProxyMut(
      { id: editEntry.id, data: editForm },
      {
        onSuccess: () => {
          toast.success('代理已更新')
          setEditEntry(null)
        },
        onError: (err) => toast.error(`更新失败: ${extractErrorMessage(err)}`),
      }
    )
  }

  const handleDelete = (entry: ProxyPoolEntry) => {
    if (!confirm(`确定要删除代理 "${entry.name}" 吗？`)) return
    deleteProxyMut(entry.id, {
      onSuccess: () => toast.success(`代理 "${entry.name}" 已删除`),
      onError: (err) => toast.error(`删除失败: ${extractErrorMessage(err)}`),
    })
  }

  const handleToggleEnabled = (entry: ProxyPoolEntry) => {
    setEnabledMut(
      { id: entry.id, enabled: !entry.enabled },
      {
        onError: (err) => toast.error(`操作失败: ${extractErrorMessage(err)}`),
      }
    )
  }

  const handleCheck = (id: number) => {
    checkProxyMut(id, {
      onSuccess: (result) => {
        if (result.healthy) {
          toast.success(`检测通过 (${result.latencyMs}ms)`)
        } else {
          toast.error('检测失败：代理不可达')
        }
      },
      onError: (err) => toast.error(`检测失败: ${extractErrorMessage(err)}`),
    })
  }

  const openEditDialog = (entry: ProxyPoolEntry) => {
    setEditEntry(entry)
    setEditForm({
      name: entry.name,
      url: entry.url,
      username: entry.username,
      password: entry.password,
    })
  }

  // 智能解析代理字符串，支持多种格式：
  // user:pass@host:port / http://user:pass@host:port / host:port:user:pass
  const parseProxyString = (raw: string): { url: string; username?: string; password?: string } | null => {
    const s = raw.trim()
    if (!s) return null

    // 带协议前缀: http://user:pass@host:port 或 socks5://user:pass@host:port
    const protoMatch = s.match(/^(https?|socks5):\/\/(?:([^:]+):([^@]+)@)?(.+)$/)
    if (protoMatch) {
      const [, proto, user, pass, hostPort] = protoMatch
      return {
        url: `${proto}://${hostPort}`,
        username: user || undefined,
        password: pass || undefined,
      }
    }

    // host:port:user:pass 格式（4 段冒号分隔，端口是纯数字）
    const parts = s.split(':')
    if (parts.length === 4 && /^\d+$/.test(parts[1])) {
      return {
        url: `http://${parts[0]}:${parts[1]}`,
        username: parts[2],
        password: parts[3],
      }
    }

    // user:pass@host:port（无协议前缀）
    const atMatch = s.match(/^([^:]+):([^@]+)@(.+)$/)
    if (atMatch) {
      const [, user, pass, hostPort] = atMatch
      return {
        url: `http://${hostPort}`,
        username: user,
        password: pass,
      }
    }

    // host:port（纯地址，无认证）
    if (parts.length === 2 && /^\d+$/.test(parts[1])) {
      return { url: `http://${s}` }
    }

    return null
  }

  const handleProxyUrlInput = (value: string, target: 'add' | 'edit') => {
    // 如果看起来像需要解析的格式（包含 @ 或 4 段冒号），尝试解析
    const parsed = parseProxyString(value)
    if (parsed && (value.includes('@') || value.split(':').length === 4)) {
      if (target === 'add') {
        setAddForm({ ...addForm, url: parsed.url, username: parsed.username, password: parsed.password })
      } else {
        setEditForm({ ...editForm, url: parsed.url, username: parsed.username, password: parsed.password })
      }
      toast.success('已自动识别代理地址和认证信息')
    } else {
      if (target === 'add') {
        setAddForm({ ...addForm, url: value })
      } else {
        setEditForm({ ...editForm, url: value })
      }
    }
  }

  const maskUrl = (url: string) => {
    try {
      const u = new URL(url)
      if (u.password) {
        u.password = '***'
      }
      return u.toString()
    } catch {
      return url
    }
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
      </div>
    )
  }

  return (
    <>
      {/* 统计卡片 */}
      <div className="grid gap-4 md:grid-cols-4 mb-6">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">代理总数</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{totalCount}</div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">健康</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold text-green-600">{healthyCount}</div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">异常</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold text-red-600">{unhealthyCount}</div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">已禁用</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold text-muted-foreground">{disabledCount}</div>
          </CardContent>
        </Card>
      </div>

      {/* 代理列表 */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-xl font-semibold">代理池管理</h2>
          <Button onClick={() => setAddDialogOpen(true)} size="sm">
            <Plus className="h-4 w-4 sm:mr-2" />
            <span className="hidden sm:inline">添加代理</span>
          </Button>
        </div>

        {totalCount === 0 ? (
          <Card>
            <CardContent className="py-8 text-center text-muted-foreground">
              暂无代理，点击"添加代理"开始配置
            </CardContent>
          </Card>
        ) : (
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            {proxies?.map((entry) => (
              <Card key={entry.id} className={!entry.enabled ? 'opacity-60' : ''}>
                <CardContent className="pt-4 space-y-3">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2 min-w-0">
                      <span className="font-medium truncate">{entry.name}</span>
                      <Badge variant="outline" className="text-xs shrink-0">#{entry.id}</Badge>
                    </div>
                    <div className="flex items-center gap-1 shrink-0">
                      {entry.enabled ? (
                        entry.healthy ? (
                          <Badge variant="success" className="gap-1">
                            <Wifi className="h-3 w-3" />健康
                          </Badge>
                        ) : (
                          <Badge variant="destructive" className="gap-1">
                            <WifiOff className="h-3 w-3" />异常
                          </Badge>
                        )
                      ) : (
                        <Badge variant="secondary" className="gap-1">
                          <PowerOff className="h-3 w-3" />禁用
                        </Badge>
                      )}
                    </div>
                  </div>

                  <div className="text-sm text-muted-foreground break-all font-mono">
                    {maskUrl(entry.url)}
                  </div>

                  {entry.username && (
                    <div className="text-xs text-muted-foreground">
                      认证: {entry.username}
                    </div>
                  )}

                  <div className="flex items-center gap-3 text-xs text-muted-foreground">
                    {entry.latencyMs != null && (
                      <span className="flex items-center gap-1">
                        <Activity className="h-3 w-3" />{entry.latencyMs}ms
                      </span>
                    )}
                    {entry.lastCheckAt && (
                      <span className="flex items-center gap-1">
                        <Clock className="h-3 w-3" />
                        {new Date(entry.lastCheckAt).toLocaleTimeString()}
                      </span>
                    )}
                    {entry.consecutiveFailures > 0 && (
                      <span className="text-red-500">连续失败 {entry.consecutiveFailures} 次</span>
                    )}
                  </div>

                  <div className="flex items-center justify-between pt-2 border-t">
                    <Switch
                      checked={entry.enabled}
                      onCheckedChange={() => handleToggleEnabled(entry)}
                    />
                    <div className="flex items-center gap-1">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8"
                        onClick={() => handleCheck(entry.id)}
                        disabled={isChecking}
                        title="手动检测"
                      >
                        <Activity className="h-4 w-4" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8"
                        onClick={() => openEditDialog(entry)}
                        title="编辑"
                      >
                        <Pencil className="h-4 w-4" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8 text-destructive"
                        onClick={() => handleDelete(entry)}
                        title="删除"
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>
        )}
      </div>

      {/* 添加对话框 */}
      <Dialog open={addDialogOpen} onOpenChange={setAddDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>添加代理</DialogTitle>
            <DialogDescription>添加新的代理到代理池</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <label className="text-sm font-medium">名称</label>
              <Input
                placeholder="如 US-West-1"
                value={addForm.name}
                onChange={(e) => setAddForm({ ...addForm, name: e.target.value })}
              />
            </div>
            <div className="space-y-2">
              <label className="text-sm font-medium">代理 URL</label>
              <Input
                placeholder="支持粘贴 user:pass@host:port 自动识别"
                value={addForm.url}
                onChange={(e) => handleProxyUrlInput(e.target.value, 'add')}
              />
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <label className="text-sm font-medium">用户名（可选）</label>
                <Input
                  placeholder="用户名"
                  value={addForm.username ?? ''}
                  onChange={(e) => setAddForm({ ...addForm, username: e.target.value || undefined })}
                />
              </div>
              <div className="space-y-2">
                <label className="text-sm font-medium">密码（可选）</label>
                <Input
                  type="password"
                  placeholder="密码"
                  value={addForm.password ?? ''}
                  onChange={(e) => setAddForm({ ...addForm, password: e.target.value || undefined })}
                />
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setAddDialogOpen(false)}>取消</Button>
            <Button onClick={handleAdd} disabled={isAdding}>
              {isAdding ? '添加中...' : '添加'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 编辑对话框 */}
      <Dialog open={editEntry !== null} onOpenChange={(open) => !open && setEditEntry(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>编辑代理</DialogTitle>
            <DialogDescription>修改代理 #{editEntry?.id} 的配置</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <label className="text-sm font-medium">名称</label>
              <Input
                value={editForm.name ?? ''}
                onChange={(e) => setEditForm({ ...editForm, name: e.target.value })}
              />
            </div>
            <div className="space-y-2">
              <label className="text-sm font-medium">代理 URL</label>
              <Input
                value={editForm.url ?? ''}
                onChange={(e) => handleProxyUrlInput(e.target.value, 'edit')}
              />
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <label className="text-sm font-medium">用户名</label>
                <Input
                  placeholder="留空清除"
                  value={editForm.username ?? ''}
                  onChange={(e) => setEditForm({ ...editForm, username: e.target.value || null })}
                />
              </div>
              <div className="space-y-2">
                <label className="text-sm font-medium">密码</label>
                <Input
                  type="password"
                  placeholder="留空清除"
                  value={editForm.password ?? ''}
                  onChange={(e) => setEditForm({ ...editForm, password: e.target.value || null })}
                />
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditEntry(null)}>取消</Button>
            <Button onClick={handleEdit} disabled={isUpdating}>
              {isUpdating ? '保存中...' : '保存'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
