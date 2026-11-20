# 安裝精靈側邊圖：41x78 格 ×4 = 164x312，補到 164x314。純自繪，不用 Mojang 素材。
import sys, random
from PIL import Image, ImageDraw
W,H,S=41,78,4
INK=(27,27,27); STONE=(169,175,181); DARK=(125,132,138); DOOR=(90,58,32); FLAG=(232,65,60)
def rect(d,x0,y0,x1,y1,c): d.rectangle([x0,y0,x1,y1],fill=c)
def castle(im,d,x0,x1,top,base,glow):
    # 主體剪影：中塔、左右塔、城牆；先畫剪影再描邊
    sil=Image.new('1',(W,H),0); s=ImageDraw.Draw(sil)
    cx=(x0+x1)//2
    s.rectangle([x0+5,top+12,x1-5,base],fill=1)           # 城牆
    s.rectangle([cx-5,top,cx+5,base],fill=1)               # 中塔
    s.rectangle([x0,top+7,x0+8,base],fill=1)               # 左塔
    s.rectangle([x1-8,top+7,x1,base],fill=1)               # 右塔
    # 垛口：挖掉間隔格
    for (a,b,y) in [(cx-5,cx+5,top),(x0,x0+8,top+7),(x1-8,x1,top+7),(x0+9,x1-9,top+12)]:
        for x in range(a,b+1):
            if (x-a)%4 in (1,2): s.rectangle([x,y,x,y+1],fill=0)
    px=sil.load(); out=set(); body=set()
    for y in range(H):
        for x in range(W):
            if px[x,y]: body.add((x,y))
    for (x,y) in body:
        for dx,dy in ((1,0),(-1,0),(0,1),(0,-1)):
            n=(x+dx,y+dy)
            if n not in body and 0<=n[0]<W and 0<=n[1]<H: out.add(n)
    for (x,y) in body: im.putpixel((x,y),STONE)
    # 磚縫（深灰點）
    for (x,y) in body:
        if (y%4==0 and (x+y//4*2)%5==0) : im.putpixel((x,y),DARK)
    for (x,y) in out: im.putpixel((x,y),INK)
    for lx in (x0+9,x1-9):
        rect(d,lx,top+14,lx,base,INK)
    # 窗
    for wx,wy in [(cx-1,top+4),(x0+3,top+13),(x1-4,top+13),(cx-10,base-9),(cx+9,base-9)]:
        rect(d,wx,wy,wx+1,wy+2,glow); 
        d.point((wx,wy-1),fill=INK); d.point((wx+1,wy-1),fill=INK)
        rect(d,wx-1,wy,wx-1,wy+2,INK); rect(d,wx+2,wy,wx+2,wy+2,INK); d.point((wx,wy+3),fill=INK); d.point((wx+1,wy+3),fill=INK)
    # 門
    rect(d,cx-3,base-6,cx+3,base,INK); rect(d,cx-2,base-5,cx+2,base,DOOR)
    d.point((cx-2,base-5),fill=INK); d.point((cx+2,base-5),fill=INK)
    # 旗
    rect(d,cx,top-8,cx,top-1,INK); rect(d,cx+1,top-8,cx+4,top-6,FLAG); d.point((cx+5,top-7),fill=FLAG)
    rect(d,cx+1,top-5,cx+3,top-5,INK) if False else None

def sky(im,d,stops):
    # stops: [(y_end,color)] 色帶式天空（像素風漸層）
    y=0
    for ye,c in stops:
        rect(d,0,y,W-1,ye,c); y=ye+1
def dither(d,y,c1,c2):
    for x in range(W):
        d.point((x,y),fill=c1 if x%2==0 else c2)
        d.point((x,y+1),fill=c2 if x%2==0 else c1)

def draft_a():
    im=Image.new('RGB',(W,H),(0,0,0)); d=ImageDraw.Draw(im)
    NAVY=(18,24,56); N2=(30,34,80); N3=(52,46,104); N4=(110,64,120); N5=(214,112,92); N6=(244,170,96)
    sky(im,d,[(11,NAVY),(22,N2),(32,N3),(40,N4),(46,N5),(52,N6)])
    dither(d,12,NAVY,N2); dither(d,23,N2,N3); dither(d,33,N3,N4); dither(d,41,N4,N5); dither(d,47,N5,N6)
    random.seed(7)
    for _ in range(26):
        x=random.randint(1,W-2); y=random.randint(1,30); d.point((x,y),fill=(255,255,255) if random.random()<.6 else (190,200,255))
    for sx,sy in [(8,9),(31,17),(18,5)]:  # 十字大星
        for dx,dy in((0,0),(1,0),(-1,0),(0,1),(0,-1)): d.point((sx+dx,sy+dy),fill=(255,248,200))
    # 月亮
    for y in range(-4,5):
        for x in range(-4,5):
            if x*x+y*y<=17 and (x-2)**2+(y+1)**2>=10: im.putpixel((30+x,9+y),(250,240,190))
    # 遠山
    for x in range(W):
        h=[0,1,2,3,3,4,5,5,4,3,3,2,3,4,5,6,6,5,4,3,2,2,3,4,5,5,4,3,2,1,2,3,4,4,3,2,1,1,2,3,3][x]
        rect(d,x,52-h,x,60,(52,40,88))
    rect(d,0,61,W-1,H-1,(34,92,52))
    for x in range(W): d.point((x,61),fill=(70,148,72)); 
    for y in range(64,H):
        for x in range(W):
            if (x*3+y*7)%11==0: d.point((x,y),fill=(26,74,44))
    rect(d,0,70,W-1,H-1,(98,64,40)); 
    for x in range(W): d.point((x,70),fill=(70,148,72)); d.point((x,71),fill=(70,148,72) if x%2==0 else (98,64,40))
    for y in range(72,H):
        for x in range(W):
            if (x*5+y*3)%9==0: d.point((x,y),fill=(74,46,28))
    castle(im,d,4,36,36,63,(255,214,92))
    return im

def draft_b():
    im=Image.new('RGB',(W,H),(0,0,0)); d=ImageDraw.Draw(im)
    B1=(104,176,236); B2=(138,198,244); B3=(176,218,248); B4=(214,236,250)
    sky(im,d,[(14,B1),(30,B2),(44,B3),(56,B4)])
    dither(d,15,B1,B2); dither(d,31,B2,B3); dither(d,45,B3,B4)
    # 太陽
    for y in range(-3,4):
        for x in range(-3,4):
            if x*x+y*y<=10: im.putpixel((7+x,9+y),(255,226,96))
    # 雲
    def cloud(x,y,w):
        rect(d,x+1,y,x+w-2,y+2,(255,255,255)); rect(d,x+3,y-1,x+w-5,y,(255,255,255)); rect(d,x,y+1,x+w-1,y+2,(255,255,255))
        rect(d,x+1,y+3,x+w-2,y+3,(214,230,244))
    cloud(18,6,12); cloud(27,19,10); cloud(3,26,9)
    # 兩層草丘
    for x in range(W):
        h1=[3,3,4,4,5,5,5,4,4,3,3,2,2,2,3,3,4,4,5,5,5,4,3,3,2,2,3,4,4,5,5,4,4,3,3,3,2,2,3,3,4][x]
        rect(d,x,56-h1,x,H-1,(104,172,76))
    for x in range(W):
        h2=[1,1,2,2,3,3,2,2,1,1,1,2,2,3,3,3,2,2,1,1,1,2,3,3,3,2,2,1,1,2,2,3,3,2,2,1,1,1,2,2,2][x]
        rect(d,x,62-h2,x,H-1,(72,148,60))
    rect(d,0,66,W-1,H-1,(60,128,52))
    for y in range(66,H):
        for x in range(W):
            if (x*7+y*5)%13==0: d.point((x,y),fill=(48,108,44))
    rect(d,0,72,W-1,H-1,(122,84,52)); 
    for x in range(W): d.point((x,72),fill=(60,128,52))
    for y in range(73,H):
        for x in range(W):
            if (x*5+y*3)%9==0: d.point((x,y),fill=(92,60,36))
    # 花
    for fx,fy,c in [(5,68,FLAG),(11,70,(255,226,96)),(30,69,FLAG),(36,68,(255,255,255))]:
        d.point((fx,fy),fill=c); d.point((fx,fy+1),fill=(48,108,44))
    castle(im,d,6,34,34,65,(60,70,96))
    return im

def out(im,name):
    big=im.resize((W*S,H*S),Image.NEAREST)
    full=Image.new('RGB',(164,314),im.getpixel((0,H-1)))
    full.paste(big,(0,0)); full.save(name)
    return full
if __name__=='__main__':
    a=out(draft_a(),'draft_A.bmp'); b=out(draft_b(),'draft_B.bmp')
    # 預覽：兩張並排＋放大
    sheet=Image.new('RGB',(164*2*2+60,314*2+40),(236,236,236))
    sheet.paste(a.resize((328,628),Image.NEAREST),(20,20)); sheet.paste(b.resize((328,628),Image.NEAREST),(368,20))
    sheet.save('preview_AB.png'); a.save('draft_A.png'); b.save('draft_B.png')
